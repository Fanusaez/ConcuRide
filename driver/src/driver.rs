use std::cmp::PartialEq;
use std::collections::HashMap;
use std::{io, thread};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use actix::{Actor, Addr, AsyncContext, StreamHandler};
use futures::SinkExt;
use tokio::io::{split, AsyncBufReadExt, BufReader, AsyncWriteExt, WriteHalf, AsyncReadExt, ReadHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio_stream::wrappers::LinesStream;

use crate::init;
use crate::models::*;
use crate::utils::*;
use crate::ride_manager::*;

pub enum Sates {
    Driving,
    Idle,
}

impl PartialEq for Sates {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Sates::Driving, Sates::Driving) => true,
            (Sates::Idle, Sates::Idle) => true,
            _ => false,
        }
    }
}


const LIDER_PORT_IDX : usize = 0;

pub struct Driver {
    /// The port of the driver
    pub id: u16,
    /// The driver's position
    pub position: (i32, i32),
    /// Whether the driver is the leader
    pub is_leader: Arc<RwLock<bool>>,
    /// Leader port
    pub leader_port: Arc<RwLock<u16>>,
    /// write half
    pub write_half_to_leader: Arc<RwLock<Option<WriteHalf<TcpStream>>>>,
    /// The connections to the drivers
    pub active_drivers: Arc<RwLock<HashMap<u16, (Option<ReadHalf<TcpStream>>, Option<WriteHalf<TcpStream>>)>>>,
    /// States of the driver
    pub state: Sates,
    /// Last known position of the driver (port, (x, y))
    pub drivers_last_position: Arc<RwLock<HashMap<u16, (i32, i32)>>>,
    /// Connection to the payment app
    pub payment_write_half: Arc<RwLock<Option<WriteHalf<TcpStream>>>>,
    /// Ride manager, contains the pending rides, unpaid rides and the rides and offers
    pub ride_manager: RideManager,
    /// ID's and WriteHalf of passengers
    pub passengers_write_half: Arc<RwLock<HashMap<u16, Option<WriteHalf<TcpStream>>>>>,
}

impl Driver {

    /// Creates the actor and starts listening for incoming passengers
    /// # Arguments
    /// * `port` - The port of the driver
    /// * `drivers_ports` - The list of driver ports TODO (leader should try to connect to them)
    pub async fn start(port: u16, mut drivers_ports: Vec<u16>, position: (i32, i32)) -> Result<(), io::Error> {
        // Driver-leader attributes
        let should_be_leader = port == drivers_ports[LIDER_PORT_IDX];
        let is_leader = Arc::new(RwLock::new(should_be_leader));
        let leader_port = Arc::new(RwLock::new(drivers_ports[LIDER_PORT_IDX].clone()));

        // Auxiliar structures
        let mut active_drivers: HashMap<u16, (Option<ReadHalf<TcpStream>>, Option<WriteHalf<TcpStream>>)> = HashMap::new();
        let mut drivers_last_position: HashMap<u16, (i32, i32)> = HashMap::new();
        let pending_rides: Arc<RwLock<HashMap<u16, RideRequest>>> = Arc::new(RwLock::new(HashMap::new()));
        let ride_and_offers: Arc::<RwLock<HashMap<u16, Vec<u16>>>> = Arc::new(RwLock::new(HashMap::new()));
        let passengers_write_half: HashMap<u16, Option<WriteHalf<TcpStream>>> = HashMap::new();

        // Payment app and connection
        let mut payment_write_half: Option<WriteHalf<TcpStream>> = None;
        let mut payment_read_half: Option<ReadHalf<TcpStream>> = None;

        let unpaid_rides: Arc<RwLock<HashMap<u16, RideRequest>>>= Arc::new(RwLock::new(HashMap::new()));

        // Remove the leader port from the list of drivers
        drivers_ports.remove(LIDER_PORT_IDX);

        init::init_driver(&mut active_drivers, drivers_ports,
                          &mut drivers_last_position,
                          should_be_leader,
                          &mut payment_write_half,
                          &mut payment_read_half).await?;

        // Arcs for shared data
        let mut active_drivers_arc = Arc::new(RwLock::new(active_drivers));
        let mut drivers_last_position_arc = Arc::new(RwLock::new(drivers_last_position));
        let mut payment_write_half_arc = Arc::new(RwLock::new(payment_write_half));
        let mut payment_read_half_arc = Arc::new(RwLock::new(payment_read_half));
        let mut passengers_write_half_arc = Arc::new(RwLock::new(passengers_write_half));
        let mut half_write_to_leader = Arc::new(RwLock::new(None));

        // para que funcione, no se porque
        let associate_driver_streams = Self::associate_drivers_stream;
        let associate_payment_stream = Self::associate_payment_stream;
        let handle_passenger_connection = Self::handle_passenger_connection;
        let handle_leader_connection = Self::handle_leader_connection;


        let driver = Driver::create(|ctx| {
            /// asocio todos los reads de los drivers al lider
            if should_be_leader {
                // Asociar streams de los conductores
                associate_driver_streams(ctx, active_drivers_arc.clone());

                // Asociar el stream del servicio de pagos
                associate_payment_stream(ctx, payment_read_half_arc.clone());
            }
            Driver {
                id: port,
                position, // Arrancan en el origen por comodidad, ver despues que onda
                is_leader: is_leader.clone(),
                leader_port: leader_port.clone(),
                active_drivers: active_drivers_arc.clone(),
                write_half_to_leader: half_write_to_leader.clone(),
                passengers_write_half: passengers_write_half_arc.clone(),
                state: Sates::Idle,
                drivers_last_position: drivers_last_position_arc.clone(),
                payment_write_half: payment_write_half_arc.clone(),
                ride_manager: RideManager {
                    pending_rides: pending_rides.clone(),
                    unpaid_rides: unpaid_rides.clone(),
                    ride_and_offers: ride_and_offers.clone(),
                },

            }
        });

        let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).await?;
        println!("WAITING FOR PASSENGERS TO CONNECT (leader) OR ACCEPTING LEADER (drivers)\n");

        while let Ok((stream, addr)) = listener.accept().await {
            if should_be_leader {
                handle_passenger_connection(stream, addr.port(), &passengers_write_half_arc, &driver)?;
            } else {
                handle_leader_connection(stream, &half_write_to_leader, &driver)?;
            }
        }
        Ok(())
    }

    /// Associates the drivers streams to the driver actor, only used by the leader
    /// # Arguments
    /// * `ctx` - The context of the driver
    /// * `active_drivers_arc` - The arc of the active drivers
    pub fn associate_drivers_stream(
        ctx: &mut actix::Context<Driver>,
        active_drivers_arc: Arc<RwLock<HashMap<u16, (Option<ReadHalf<TcpStream>>, Option<WriteHalf<TcpStream>>)>>>)
        -> Result<(), io::Error>
    {
        let mut active_drivers = active_drivers_arc.write().unwrap();

        for (id, (read, _)) in active_drivers.iter_mut() {
            if let Some(read_half) = read.take() {
                Driver::add_stream(LinesStream::new(BufReader::new(read_half).lines()), ctx);
            } else {
                eprintln!("No hay un stream de lectura disponible para el conductor con id {}", id);
            }
        }
        Ok(())
    }

    /// Associates the payment stream to the driver actor, only used by the leader
    /// # Arguments
    /// * `ctx` - The context of the driver
    /// * `payment_read_half_arc` - The arc of the payment read half
    pub fn associate_payment_stream(
        ctx: &mut actix::Context<Driver>,
        payment_read_half_arc: Arc<RwLock<Option<ReadHalf<TcpStream>>>>)
        -> Result<(), io::Error> {
        let mut payment_read_half = payment_read_half_arc.write().unwrap();

        if let Some(payment_read_half) = payment_read_half.take() {
            Driver::add_stream(LinesStream::new(BufReader::new(payment_read_half).lines()), ctx);
        } else {
            eprintln!("No hay un stream de lectura disponible para el servicio de pagos");
        }
        Ok(())
    }

    /// Manages the connection of a passenger, only used by the leader
    /// # Arguments
    /// * `stream` - The stream of the passenger
    /// * `id` - The id of the passenger
    /// * `passengers_write_half_arc` - The arc of the write halves of the passengers
    /// * `driver` - The address of the driver
    fn handle_passenger_connection(
        stream: TcpStream,
        port_used: u16,
        passengers_write_half_arc: &Arc<RwLock<HashMap<u16, Option<WriteHalf<TcpStream>>>>>,
        driver: &Addr<Driver>,
    ) -> Result<(), io::Error> {
        let (read, write) = split(stream);

        // Guardar el WriteHalf del pasajero
        let mut passengers_write_half = passengers_write_half_arc.write().unwrap();

        passengers_write_half.insert(port_used, Some(write));

        // Agregar el stream de lectura al actor
        if driver.connected() {
            match driver.try_send(StreamMessage { stream: Some(read) }) {
                Ok(_) => {}
                Err(e) => eprintln!("Error al enviar el stream al actorrrrr: {:?}", e),
            }
        } else {
            eprintln!("El actor Driver ya no está activo.");
        }
        Ok(())
    }

    /// Manages the connection of a leader, only used by the drivers
    /// # Arguments
    /// * `stream` - The stream of the leader
    /// * `half_write_to_leader` - The arc of the write half to the leader
    /// * `driver` - The address of the driver
    fn handle_leader_connection(
        stream: TcpStream,
        half_write_to_leader: &Arc<RwLock<Option<WriteHalf<TcpStream>>>>,
        driver: &Addr<Driver>,
    ) -> Result<(), io::Error> {
        let (read, write) = split(stream);

        // Agregar el stream de lectura al actor
        match driver.try_send(StreamMessage { stream: Some(read) }) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("Error al enviar el stream al actor: {:?}", e);
            }
        }

        // Guardar el WriteHalf del líder
        half_write_to_leader.write().unwrap().replace(write);
        Ok(())
    }

/// -------------------------------- Fin del start/inicializacion -------------------------------- ///


    /// Handles the ride request from the leader as a driver
    /// # Arguments
    /// * `msg` - The message containing the ride request
    pub fn handle_ride_request_as_driver(&mut self, msg: RideRequest, addr: Addr<Self>) -> Result<(), io::Error> {
        let probability = 0.99;
        let result = boolean_with_probability(probability);

        if result && self.state == Sates::Idle {
            println!("Driver {} accepted the ride request", self.id);
            self.accept_ride_request(msg)?;
            self.drive_and_finish(msg, addr)?;
        } else {
            self.decline_ride_request(msg)?;
            println!("Driver {} rejected the ride request", self.id);
        }
        Ok(())

    }

    /// Handles the ride request from passenger, sends RideRequest to the closest driver
    /// # Arguments
    /// * `msg` - The message containing the ride request
    pub fn handle_ride_request_as_leader(&mut self, msg: RideRequest) -> Result<(), io::Error> {

        // Si ya hay un RideRequest en proceso, se ignora
        // TODO: quizas deberiamos prohibir esto en el pasajero
        if self.ride_manager.has_pending_ride_request(msg.id)? {
            // Ignorar si ya hay un RideRequest en proceso
            return Ok(());
        }

        // lo agrego a los unpaid rides
        self.ride_manager.insert_unpaid_ride(msg).expect("Error adding unpaid ride");

        // envio el pago a la app
        self.send_payment(msg).expect("Error sending payment");

        Ok(())
    }

    /// Handles the payment accepted message from the payment app
    /// Sends the ride request to the closest driver and adds the driver to the offers
    /// # Arguments
    /// * `msg` - The message containing the PaymentAccepted
    pub fn handle_payment_accepted_as_leader(&mut self, msg: PaymentAccepted, addr: Addr<Self>) -> Result<(), io::Error> {
        // obtengo el ride request del unpaid_rides y lo elimino del hashmap
        let ride_request = self.ride_manager.remove_unpaid_ride(msg.id)?;

        /// saves ride in pending_rides
        self.ride_manager.insert_ride_in_pending(ride_request)?;

        /// Busco el driver mas cercano y le envio el RideRequest
        self.search_driver_and_send_ride(ride_request, addr)?;

        Ok(())
    }

    /// Search the closest driver to the passenger and send the ride request
    /// Insert the driver id in the ride_and_offers hashmap
    /// # Arguments
    /// * `ride_request` - The ride request to send
    fn search_driver_and_send_ride(&mut self, ride_request: RideRequest, addr: Addr<Self>) -> Result<(), io::Error> {
        /// Logica de a quien se le manda el mensaje
        /// TODO: Ojo que puede devlver cero, hay que ver que hacer ahi
        let driver_id_to_send = self.get_closest_driver(ride_request);

        println!("HOLAAAA");

        /// Si no hay drivers disponibles
        if driver_id_to_send == 0 {
            println!("No hay drivers disponibles para el pasajero con id {}, se intentara mas tarde", ride_request.id);

            // Elimino todas las ofertas que se hicieron
            self.ride_manager.remove_from_ride_and_offers(ride_request.id)?;

            // Pauso y reinicio la busqueda de drivers
            self.pause_and_restart_driver_search(ride_request,addr)?;

            return Ok(());
        }

        /// Envio el mensaje al driver
        self.send_ride_request_to_driver(driver_id_to_send, ride_request)?;

        /// Agrego el id del driver al vector de ofertas
        self.ride_manager.insert_in_rides_and_offers(ride_request.id, driver_id_to_send)?;

        Ok(())
    }

    /// Pause and restart the driver search, so we dont do a busy wait
    fn pause_and_restart_driver_search(&mut self, ride_request: RideRequest, addr: Addr<Self>) -> Result<(), io::Error> {
        let ride_request_clone = ride_request.clone();

        actix::spawn(async move {

            println!("PAUSAMOS LA BUSQUEDA");
            // Simula espera
            tokio::time::sleep(Duration::from_secs(3)).await;

            // TODO: Crear un nuevo mensaje para este proposito
            let response = DeclineRide {
                passenger_id: ride_request_clone.id,
                driver_id: 0,
            };

            addr.do_send(response);

        });
        Ok(())
    }

    /// Handles the payment rejected message from the payment app
    /// Sends the payment rejected message to the passenger
    pub fn handle_payment_rejected_as_leader(&mut self, msg: PaymentRejected) -> Result<(), io::Error> {
        self.send_message_to_passenger(MessageType::PaymentRejected(msg), msg.id)?;
        Ok(())
    }

    /// Handles the accept ride message from the driver
    /// Removes the passenger id from ride_and_offers
    pub fn handle_accept_ride_as_leader(&mut self, msg: AcceptRide) -> Result<(), io::Error> {
        // Remove the passenger ID from ride_and_offers in case the passenger wants to take another ride
        if let Err(e) = self.ride_manager.remove_from_ride_and_offers(msg.passenger_id) {
            eprintln!(
                "Error removing passenger ID {} from ride_and_offers: {:?}",
                msg.passenger_id, e
            );
        }
        Ok(())
    }

    /// Handles the declined ride from driver as leader
    /// Sends the ride request to the next closest driver and adds the driver to the offers
    /// # Arguments
    /// * `msg` - The message containing the Declined Ride
    pub fn handle_declined_ride_as_leader(&mut self, msg: DeclineRide, addr: Addr<Self>) -> Result<(), io::Error> {
        let ride_request = self.ride_manager.get_pending_ride_request(msg.passenger_id)?;

        // vuelvo a buscar el driver mas cercano
        self.search_driver_and_send_ride(ride_request, addr)?;

        Ok(())
    }

    /// Handles the FinishRide message from the driver
    /// Removes the ride from the pending rides and sends the FinishRide message to the passenger
    /// # Arguments
    /// * `msg` - The message containing the FinishRide
    pub fn handle_finish_ride_as_leader(&mut self, msg: FinishRide) -> Result<(), io::Error> {
        // Remove the ride from the pending rides
        self.ride_manager.remove_ride_from_pending(msg.passenger_id)?;

        let msg_message_type = MessageType::FinishRide(msg);

        // Send the FinishRide message to the passenger
        self.send_message_to_passenger(msg_message_type, msg.passenger_id)?;

        Ok(())
    }

    /// Finish the ride, send the FinishRide message to the leader
    /// # Arguments
    /// * `msg` - The message containing the ride request
    pub fn handle_finish_ride_as_driver(&mut self, msg: FinishRide) -> Result<(), io::Error> {
        println!("Driver is now im position: {:?}", self.position);

        // Cambiar el estado del driver a Idle
        self.state = Sates::Idle;

        // Enviar el mensaje de finalización al líder
        self.send_message_to_leader(MessageType::FinishRide(msg))?;

        Ok(())
    }

    /// Sends the ride request to the driver specified by the id, only used by the leader
    /// # Arguments
    /// * `driver_id` - The id of the driver
    /// * `msg` - The message containing the ride request
    pub fn send_ride_request_to_driver(&mut self, driver_id: u16, msg: RideRequest) -> Result<(), io::Error> {
        let mut active_drivers_clone = Arc::clone(&self.active_drivers);
        let msg_clone = msg.clone();

        actix::spawn(async move {
            let mut active_drivers = match active_drivers_clone.write() {
                Ok(guard) => guard,
                Err(e) => {
                    eprintln!("Error al obtener el lock de escritura en `active_drivers`: {:?}", e);
                    return;
                }
            };

            if let Some((_, write_half)) = active_drivers.get_mut(&driver_id) {
                let response = MessageType::RideRequest(msg_clone);
                let serialized = match serde_json::to_string(&response) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("Error serializando el mensaje: {:?}", e);
                        return;
                    }
                };

                if let Some(write_half) = write_half.as_mut() {
                    if let Err(e) = write_half.write_all(format!("{}\n", serialized).as_bytes()).await {
                        eprintln!("Error al enviar el mensaje: {:?}", e);
                    }
                } else {
                    eprintln!("No se pudo enviar el mensaje: no hay conexión activa");
                }
            } else {
                eprintln!("No se encontró un `write_half` para el `driver_id_to_send` especificado");
            }
        });
        Ok(())
    }

    /// Driver's function
    /// Sends the AcceptRide message to the leader
    /// # Arguments
    /// * `msg` - The message containing the ride request
    fn accept_ride_request(&mut self, ride_request_msg: RideRequest) -> Result<(), io::Error> {

        // Cambiar el estado del driver a Driving
        self.state = Sates::Driving;

        // Crear el mensaje de respuesta
        let response = AcceptRide {
            passenger_id: ride_request_msg.id,
            driver_id: self.id,
        };

        // Serializar el mensaje en JSON
        let accept_msg = MessageType::AcceptRide(response);

        // Enviar el mensaje de aceptacion de manera asíncrona
        self.send_message_to_leader(accept_msg)?;

        Ok(())
    }

    /// Driver's function
    /// Sends the DeclineRide message to the leader
    /// # Arguments
    /// * `msg` - The message containing the ride request
    fn decline_ride_request(&self, msg: RideRequest) -> Result<(), io::Error> {

        // Crear el mensaje de respuesta
        let response = DeclineRide {
            passenger_id: msg.id,
            driver_id: self.id,
        };

        // Serializar el mensaje en JSON
        let msg_type = MessageType::DeclineRide(response);

        // Enviar el mensaje de manera asíncrona
        self.send_message_to_leader(msg_type)?;

        Ok(())
    }

    /// Generic function to send a message to the passenger specified by the id, only used by the leader
    /// # Arguments
    /// * `message` - The message to send
    /// * `passenger_id` - The id of the passenger
    pub fn send_message_to_passenger(
        &self,
        message: MessageType,
        passenger_id: u16,
    ) -> Result<(), io::Error> {
        let mut passengers_write_half_clone =  self.passengers_write_half.clone();
        let serialized = serde_json::to_string(&message)?;

        actix::spawn(async move {
            let mut passengers_half_write = match passengers_write_half_clone.write() {
                Ok(guard) => guard,
                Err(e) => {
                    eprintln!("Error al obtener el lock de escritura en `active_drivers`: {:?}", e);
                    return;
                }
            };

            if let Some(write_half) = passengers_half_write.get_mut(&passenger_id) {
                let serialized = match serde_json::to_string(&message) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("Error serializando el mensaje: {:?}", e);
                        return;
                    }
                };

                if let Some(write_half) = write_half.as_mut() {
                    if let Err(e) = write_half.write_all(format!("{}\n", serialized).as_bytes()).await {
                        eprintln!("Error al enviar el mensaje: {:?}", e);
                    }
                } else {
                    eprintln!("No se pudo enviar el mensaje: no hay conexión activa");
                }
            } else {
                eprintln!("No se encontró un `write_half` para el `driver_id_to_send` especificado");
            }
        });
        Ok(())
    }

    /// Driver's function
    /// Sends a message to the leader
    /// # Arguments
    /// * `message` - The message to send
    pub fn send_message_to_leader(&self, message: MessageType) -> Result<(), io::Error> {
        let write_half = Arc::clone(&self.write_half_to_leader);
        let serialized = serde_json::to_string(&message)?;

        actix::spawn(async move {
            let mut write_guard = write_half.write().unwrap();

            if let Some(write_half) = write_guard.as_mut() {
                if let Err(e) = write_half.write_all(format!("{}\n", serialized).as_bytes()).await {
                    eprintln!("Error al enviar el mensaje: {:?}", e);
                }
            } else {
                eprintln!("No se pudo enviar el mensaje: no hay conexión activa");
            }
        });

        Ok(())
    }

    /// Driver's function
    /// Simulates the travel from the origin to the destination
    /// # Arguments
    /// * `msg` - The message containing the ride request
    /// * `addr` - The address of the driver
    fn drive_and_finish(&mut self, ride_request_msg: RideRequest, addr: Addr<Self>) -> Result<(), io::Error> {
        let msg_clone = ride_request_msg.clone();
        let driver_id = self.id.clone();
        let duration = calculate_travel_duration(&msg_clone);
        let position = self.position;

        actix::spawn(async move {
            let mut current_position = (msg_clone.x_origin, msg_clone.y_origin);
            let advancement = 10;  // Dividir el viaje en 10 pasos
            let x_km = (msg_clone.x_dest as i32 - msg_clone.x_origin as i32) as f64 / advancement as f64;
            let y_km = (msg_clone.y_dest as i32 - msg_clone.y_origin as i32) as f64 / advancement as f64;

            for i in 0..advancement {
                current_position.0 = (msg_clone.x_origin as f64 + x_km * i as f64) as u16;
                current_position.1 = (msg_clone.y_origin as f64 + y_km * i as f64) as u16;

                println!("Conductor {}: Actualizando posición a {:?}", driver_id, current_position);

                // Simula espera
                tokio::time::sleep(Duration::from_secs(duration / advancement as u64)).await;
            }

            // Enviar mensaje al actor para finalizar el viaje
            let finish_ride = FinishRide {
                passenger_id: ride_request_msg.id,
                driver_id,
            };

            addr.do_send(finish_ride);
        });

        // Actualiza la posición final en el actor
        self.position = (msg_clone.x_dest.into(), msg_clone.y_dest.into());

        Ok(())
    }

    /// Sends message to the payment app, requesting to make a reservation for 2399
    pub fn send_payment(&mut self, msg: RideRequest) -> Result<(), io::Error>{
        //TODO: Ver el tema de la cantidad pagada
        let message = SendPayment{id: msg.id, amount: 2399};
        self.send_message_to_payment_app(MessageType::SendPayment(message))?;
        Ok(())
    }

    /// TODO: hacer una funcion generica que reciba mensaje y canal de escritura para no repetir codigo
    fn send_message_to_payment_app(&self, message: MessageType) -> Result<(), io::Error> {
        let write_half = Arc::clone(&self.payment_write_half);
        let serialized = serde_json::to_string(&message)?;

        actix::spawn(async move {
            let mut write_guard = write_half.write().unwrap();

            if let Some(write_half) = write_guard.as_mut() {
                if let Err(e) = write_half.write_all(format!("{}\n", serialized).as_bytes()).await {
                    eprintln!("Error al enviar el mensaje: {:?}", e);
                }
            } else {
                eprintln!("No se pudo enviar el mensaje: no hay conexión activa");
            }
        });

        Ok(())
    }

    /// Checks if there is a pending ride request for the passenger
    /// If there is, sends a message to the passenger to reconnect
    /// # Arguments
    /// * `passenger_id` - The id of the passenger
    pub fn verify_pending_ride_request(&self, passenger_id: u16) -> Result<(), io::Error> {
        // Verificar si hay una solicitud pendiente
        let has_pending = self.ride_manager.has_pending_ride_request(passenger_id)?;

        if !has_pending {
            return Ok(());
        }

        // Enviar mensaje de reconexión al pasajero
        let message = RideRequestReconnection {
            passenger_id,
            state: "WaitingDriver".to_string(),
        };

        self.send_message_to_passenger(MessageType::RideRequestReconnection(message), passenger_id)?;

        Ok(())
    }

    /// -------------------------------------------  AUXILIARY FUNCTIONS ------------------------------------------- ///


    /// Returns the id to the closest driver to the passenger that has not been already offered the ride
    /// In case there is no driver available, it returns 0 (TODO: VER SI ESTO ESTA BIEN, QUIZAS HAYA QUE HACERLO MEJOR)
    /// # Arguments
    /// * `message` - The message containing the ride request
    /// # Returns
    /// The id of the closest driver
    pub fn get_closest_driver(&self, message: RideRequest) -> u16 {
        // PickUp position
        let (x_passenger, y_passenger) = (message.x_origin as i32, message.y_origin as i32);

        let drivers_last_position = self.drivers_last_position.read().unwrap();
        let mut closest_driver = 0;
        let mut min_distance = i32::MAX;

        let offers_and_rides = self.ride_manager.ride_and_offers.read().unwrap();


        for (driver_id, (x_driver, y_driver)) in drivers_last_position.iter() {

            // Si el driver que estoy viendo ya fue ofrecido el viaje, lo salteo
            // TODO: VER MANEJO DE ERRORES
            if offers_and_rides.contains_key(&message.id) && offers_and_rides.get(&message.id).unwrap().contains(driver_id) {
                continue;
            }

            let distance = (x_passenger - x_driver).abs() + (y_passenger - y_driver).abs();
            if distance < min_distance {
                min_distance = distance;
                closest_driver = *driver_id;
            }
        }
        closest_driver
    }

}