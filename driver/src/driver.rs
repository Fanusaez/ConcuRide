use std::cmp::PartialEq;
use std::collections::HashMap;
use std::{io, thread};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::Duration;
use actix::{Actor, Addr, AsyncContext, StreamHandler, Context, Handler, Message};
use actix::clock::Sleep;
use tokio::io::{split, AsyncBufReadExt, BufReader, AsyncWriteExt, WriteHalf, AsyncReadExt, ReadHalf};
use tokio::net::{TcpListener, TcpStream};
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use futures::SinkExt;
use tokio_stream::wrappers::LinesStream;
use actix::MessageResult;

use crate::{init, utils};
use crate::models::*;
use crate::utils::*;
use crate::ride_manager::*;
use std::time::Instant;

pub struct LastPingManager {
    pub last_ping: Instant,
}

pub enum States {
    Driving,
    Idle,
    LeaderLess
}

impl PartialEq for States{
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (States::Driving, States::Driving) => true,
            (States::Idle, States::Idle) => true,
            _ => false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct DriverStatus {
    pub last_response: Option<std::time::Instant>, // Último tiempo de respuesta
    pub is_alive: bool, // Estado del driver
}


const LIDER_PORT_IDX : usize = 0;
const PING_INTERVAL: u64 = 5;
const RESTART_DRIVER_SEARCH_INTERVAL: u64 = 5;
const PROBABILITY_OF_ACCEPTING_RIDE: f64 = 0.99;
const NO_DRIVER_AVAILABLE: u16 = 0;
const PING_TIMEOUT: u64 = 7;
const TIMEOUT: Duration = Duration::from_secs(5);

pub struct Driver {
    /// The port of the driver
    pub id: u16,
    /// The driver's position
    pub position: (i32, i32),
    /// Whether the driver is the leader
    pub is_leader: Arc<AtomicBool>,
    /// Leader port
    pub leader_port: u16,
    /// write half
    pub write_half_to_leader: Arc<RwLock<Option<WriteHalf<TcpStream>>>>,
    /// Liders last ping
    pub last_ping_by_leader: Addr<LastPingManager>,
    /// The connections to the drivers
    pub active_drivers: Arc<RwLock<HashMap<u16, (Option<ReadHalf<TcpStream>>, Option<WriteHalf<TcpStream>>)>>>,
    /// ID's of Dead Drivers
    pub dead_drivers: Vec<u16>,
    /// States of the driver
    pub state: States,
    /// Last known position of the driver (port, (x, y))
    pub drivers_last_position: HashMap<u16, (i32, i32)>,
    /// Connection to the payment app
    pub payment_write_half: Arc<RwLock<Option<WriteHalf<TcpStream>>>>,
    /// Ride manager, contains the pending rides, unpaid rides and the rides and offers
    pub ride_manager: RideManager,
    /// ID's and WriteHalf of passengers
    pub passengers_write_half: Arc<RwLock<HashMap<u16, Option<WriteHalf<TcpStream>>>>>,
    /// Status of the drivers
    pub drivers_status: Arc<RwLock<HashMap<u16, DriverStatus>>>,
    /// UDP Socket for leader election
    pub socket: Arc<UdpSocket>,
    /// Driver's IDS
    pub drivers_id: Vec<u16>,
    /// para parar check_leader_alive, es para cuando siendo driver me convierto en lider. todo: ver despues
    pub stop_pings: Arc<AtomicBool>,
}

impl Driver {

    /// Creates the actor and starts listening for incoming passengers
    /// # Arguments
    /// * `port` - The port of the driver
    /// * `drivers_ports` - The list of driver ports TODO (leader should try to connect to them)
    pub async fn start(port: u16, mut drivers_ports: Vec<u16>, position: (i32, i32)) -> Result<(), io::Error> {
        // Driver-leader attributes
        let should_be_leader = port == drivers_ports[LIDER_PORT_IDX];
        let is_leader = should_be_leader;
        let is_leader_arc = Arc::new(AtomicBool::new(is_leader));
        let leader_port = drivers_ports[LIDER_PORT_IDX].clone();
        let last_ping_manager = LastPingManager { last_ping: Instant::now() }.start();

        // Auxiliar structures
        let mut active_drivers: HashMap<u16, (Option<ReadHalf<TcpStream>>, Option<WriteHalf<TcpStream>>)> = HashMap::new();
        let mut drivers_last_position: HashMap<u16, (i32, i32)> = HashMap::new();
        let passengers_write_half: HashMap<u16, Option<WriteHalf<TcpStream>>> = HashMap::new();
        let mut drivers_status: HashMap<u16, DriverStatus> = HashMap::new();

        // Payment app and connection
        let mut payment_write_half: Option<WriteHalf<TcpStream>> = None;
        let mut payment_read_half: Option<ReadHalf<TcpStream>> = None;

        // Leader election (tengo pensado usar los puertos 10001, 10002, ...)
        let port_socket = port + 4000;
        let socket = UdpSocket::bind(format!("127.0.0.1:{}", port_socket))?;

        // TODO: Y ESTO?
        let unpaid_rides: Arc<RwLock<HashMap<u16, RideRequest>>>= Arc::new(RwLock::new(HashMap::new()));
        let stop_pings = Arc::new(AtomicBool::new(false));

        // Remove the leader port from the list of drivers
        drivers_ports.remove(LIDER_PORT_IDX);

        let drivers_id = drivers_ports.clone();

        init::init_driver(&mut active_drivers, drivers_ports,
                          &mut drivers_last_position,
                          should_be_leader,
                          &mut payment_write_half,
                          &mut payment_read_half,
                          &mut drivers_status).await?;

        // Arcs for shared data
        let mut active_drivers_arc = Arc::new(RwLock::new(active_drivers));
        //let mut drivers_last_position_arc = Arc::new(RwLock::new(drivers_last_position));
        let mut payment_write_half_arc = Arc::new(RwLock::new(payment_write_half));
        let mut payment_read_half_arc = Arc::new(RwLock::new(payment_read_half));
        let mut passengers_write_half_arc = Arc::new(RwLock::new(passengers_write_half));
        let mut half_write_to_leader = Arc::new(RwLock::new(None));
        let mut drivers_status_arc = Arc::new(RwLock::new(drivers_status));
        //let last_ping_by_leader = Arc::new(RwLock::new(last_ping_by_leader));

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
                is_leader: is_leader_arc.clone(),
                leader_port: leader_port.clone(),
                last_ping_by_leader: last_ping_manager, // porque le ponian clone?
                active_drivers: active_drivers_arc.clone(),
                dead_drivers: Vec::new(),
                write_half_to_leader: half_write_to_leader.clone(),
                passengers_write_half: passengers_write_half_arc.clone(),
                state: States::Idle,
                drivers_last_position,
                payment_write_half: payment_write_half_arc.clone(),
                drivers_status: drivers_status_arc.clone(),
                ride_manager: RideManager::new(),
                socket: Arc::new(socket),
                drivers_id,
                stop_pings: stop_pings.clone(),

            }
        });

        let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).await?;

        while let Ok((stream, addr)) = listener.accept().await {
            if is_leader_arc.load(Ordering::SeqCst) {
                println!("Conexión de un pasajero en {}", addr.port());
                handle_passenger_connection(stream, addr.port(), &passengers_write_half_arc, &driver)?;
            }
            else {
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
        ctx: &mut Context<Driver>,
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

/// ------------------------------------------------------------------Fin del start/inicializacion -------------------------------------------- ///


/// ------------------------------------------------------------------ PING IMPLEMENTATION ---------------------------------------------------- ///


    /// Starts the ping system
    /// In a loop sends pings to the drivers
    /// # Arguments
    /// * `addr` - The address of the driver
    pub fn start_ping_system(&self, addr: Addr<Driver>) {
        let mut drivers_status = self.drivers_status.clone();

        tokio::spawn(async move {
            loop {
                {
                    let mut drivers = drivers_status.write().unwrap();
                    for (driver_id, status) in drivers.iter_mut() {
                        if status.is_alive {
                            addr.do_send(SendPingTo { id_to_send: *driver_id });

                            // si esta muerto, me automando un mesnaje
                            if !update_driver_status(status, *driver_id) {
                                println!("Driver {} is dead", driver_id);
                                addr.do_send(DeadDriver { driver_id: *driver_id });
                            }
                        }
                    }
                }
                tokio::time::sleep(Duration::from_secs(PING_INTERVAL)).await; // Enviar pings cada 5 segundos
            }
        });
    }

    /// Handles the ping message as a driver
    /// This is a response Ping from a driver (PONG)
    pub fn handle_ping_as_leader(&self, msg: Ping) -> Result<(), io::Error> {
        // id del driver que me envio el ping
        let driver_id = msg.id_sender;

        // obtengo locks
        let mut drivers_status = self.drivers_status.write().unwrap();

        let driver_status = match drivers_status.get_mut(&driver_id) {
            Some(status) => status,
            None => {
                eprintln!("Error: No se encontró el estado del driver con id {}", driver_id);
                return Ok(());
            }
        };

        // actualizo el tiempo del ultimo mensaje recibido
        driver_status.last_response = Some(std::time::Instant::now());

        Ok(())
    }

    /// Handles the ping message as a driver
    /// This is a response Ping from a driver
    /// # Arguments
    /// * `msg` - The message containing the ping
    pub fn handle_ping_as_driver(&mut self, msg: Ping) -> Result<(), io::Error> {
        let response = Ping {
            id_sender: self.id,
            id_receiver: msg.id_sender,
        };
        // Update last ping time of leader
        self.last_ping_by_leader.do_send(UpdateLastPing { time: Instant::now() });

        // Resend ping (PONG)
        self.send_message_to_leader(MessageType::Ping(response))?;
        Ok(())
    }

    /// Sends a ping to the driver with the specified id
    pub fn send_ping_to_driver(&mut self, id_driver: u16) -> Result<(), io::Error> {
        let message = Ping {
            id_sender: self.id,
            id_receiver: id_driver,
        };
        self.send_message_to_driver(id_driver, MessageType::Ping(message))?;

        Ok(())
    }

    /// Checks if the leader is alive, if the leader stop sending pings, the driver will send an auto message
    /// Only used by driver
    pub fn check_leader_alive(&mut self, addr: Addr<Driver>) {
        let last_ping_by_leader = self.last_ping_by_leader.clone();
        let leader_id = self.leader_port;
        let stop_flag = self.stop_pings.clone();

        actix::spawn(async move {
            loop {
                if stop_flag.load(Ordering::SeqCst) {
                    // todo: ojo capaz que no hace falta, se corta solo abajo
                    println!("Deteniendo la tarea de verificación de líder.");
                    break;
                }
                let last_ping = last_ping_by_leader.send(GetLastPing).await.unwrap();

                let now = Instant::now();
                let elapsed = now.duration_since(last_ping).as_secs();

                if elapsed > PING_TIMEOUT {
                    addr.do_send(DeadLeader { leader_id });
                    break;
                }

                tokio::time::sleep(Duration::from_secs(PING_INTERVAL)).await;
            }
        });
    }


    /// ------------------------------------------------------------------ END PING IMPLEMENTATION ---------------------------------------------------- ///

    ///Handles the ride request from the leader as a driver
    /// # Arguments
    /// * `msg` - The message containing the ride request
    pub fn handle_ride_request_as_driver(&mut self, msg: RideRequest, addr: Addr<Self>) -> Result<(), io::Error> {
        let result = boolean_with_probability(PROBABILITY_OF_ACCEPTING_RIDE);

        if result && self.state == States::Idle {
            log(&format!("RIDE REQUEST {} ACCEPTED", msg.id), "DRIVER");
            self.accept_ride_request(msg)?;
            self.drive_and_finish(msg, addr)?;
        } else {
            log(&format!("RIDE REQUEST {} REJECTED", msg.id), "DRIVER");
            self.decline_ride_request(msg)?;
        }
        Ok(())

    }

    /// Handles the ride request from passenger, sends RideRequest to the closest driver
    /// # Arguments
    /// * `msg` - The message containing the ride request
    pub fn handle_ride_request_as_leader(&mut self, msg: RideRequest) -> Result<(), io::Error> {

        // Si ya hay un RideRequest en proceso, se ignora
        // TODO: quizas deberiamos prohibir esto en el pasajero
        if self.ride_manager.has_pending_ride_request(msg.id) {
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

        // Inserto el id y el pago en la lista de viajes pagos
        self.ride_manager.insert_ride_in_paid_rides(msg.id, msg)?;

        Ok(())
    }

    /// Search the closest driver to the passenger and send the ride request
    /// Insert the driver id in the ride_and_offers hashmap
    /// # Arguments
    /// * `ride_request` - The ride request to send
    fn search_driver_and_send_ride(&mut self, ride_request: RideRequest, addr: Addr<Self>) -> Result<(), io::Error> {
        let driver_id_to_send = self.get_closest_driver(ride_request);

        /// Si no hay drivers disponibles
        if driver_id_to_send == NO_DRIVER_AVAILABLE {
            log(&format!("NO DRIVERS AVAILABLE RIGHT NOW FOR PASSENGER {}, TRYING AGAIN LATER", ride_request.id), "INFO");

            // Elimino todas las ofertas que se hicieron (pero no el id del pasajero)
            self.ride_manager.remove_offers_from_ride_and_offers(ride_request.id)?;

            // Pauso y reinicio la busqueda de drivers
            self.pause_and_restart_driver_search(ride_request, addr)?;

            return Ok(());
        }

        /// Envio el mensaje al driver
        self.send_message_to_driver(driver_id_to_send, MessageType::RideRequest(ride_request))?;

        /// Agrego el id del driver al vector de ofertas
        self.ride_manager.insert_in_rides_and_offers(ride_request.id, driver_id_to_send)?;

        Ok(())
    }

    /// Pause and restart the driver search, so we dont do a busy wait
    fn pause_and_restart_driver_search(&mut self, ride_request: RideRequest, addr: Addr<Self>) -> Result<(), io::Error> {
        let ride_request_clone = ride_request.clone();

        actix::spawn(async move {

            // Simula espera
            tokio::time::sleep(Duration::from_secs(RESTART_DRIVER_SEARCH_INTERVAL)).await;

            let response = RestartDriverSearch {
                passenger_id: ride_request_clone.id,
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

    /// Handles the RestartDriverSearch message
    /// # Arguments
    /// * `msg` - The message containing the RestartDriverSearch
    pub fn handle_restart_driver_search_as_leader(&mut self, msg: RestartDriverSearch, addr: Addr<Self>) -> Result<(), io::Error> {
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

        // Pay ride to driver
        let payment = self.ride_manager.get_ride_from_paid_rides(msg.passenger_id)?;
        let pay_ride_msg = PayRide{ride_id:msg.passenger_id, amount:payment.amount};
        self.send_payment_to_driver(msg.driver_id, pay_ride_msg)?;

        Ok(())
    }

    /// Sends the payment message to the driver
    pub fn send_payment_to_driver(&mut self, driver_id: u16, msg: PayRide) -> Result<(), io::Error> {
        self.send_message_to_driver(driver_id, MessageType::PayRide(msg))
    }

    /// Finish the ride, send the FinishRide message to the leader
    /// # Arguments
    /// * `msg` - The message containing the ride request
    pub fn handle_finish_ride_as_driver(&mut self, msg: FinishRide) -> Result<(), io::Error> {
        log(&format!("RIDE FINISHED, PASSENGER WITH ID {} HAS BEEN DROPPED OFF", msg.passenger_id), "DRIVER");

        // Cambiar el estado del driver a Idle
        self.state = States::Idle;

        // Enviar el mensaje de finalización al líder
        self.send_message_to_leader(MessageType::FinishRide(msg))?;

        Ok(())
    }

    /// Handles the dead driver message from the leader
    /// Removes the driver from the active drivers and adds it to the dead drivers
    /// In case the dead driver was offered a ride and not responded, sends auto decline message, so the RideRequest can be sent to another driver
    pub fn handle_dead_driver_as_leader(&mut self, addr: Addr<Self>, msg: DeadDriver) -> Result<(), io::Error> {
        let mut active_drivers = self.active_drivers.write().unwrap();

        let dead_driver_id = msg.driver_id;

        active_drivers.remove(&dead_driver_id);
        self.dead_drivers.push(dead_driver_id);
        self.drivers_last_position.remove(&dead_driver_id);

        // verificar si en ride and offers se encuentra el driver en ultima posicion de un viaje
        for (passenger_id, drivers_id) in self.ride_manager.ride_and_offers.iter_mut() {
            if let Some(last_driver) = drivers_id.last() {
                // si el ultimo id al que se le ofrecio el viaje es el driver muerto
                // finjo una respuesta del driver muerto
                // Sino creo que tengo problemas con los locks y addr
                if *last_driver == dead_driver_id {
                    addr.do_send(DeclineRide { passenger_id: *passenger_id, driver_id: dead_driver_id });
                }
            }
        }
        Ok(())
    }

    /// Sends the ride request to the driver specified by the id, only used by the leader
    /// # Arguments
    /// * `driver_id` - The id of the driver
    /// * `msg` - The message containing the ride request
    pub fn send_ride_request_to_driver(&mut self, driver_id: u16, msg: RideRequest) -> Result<(), io::Error> {
        self.send_message_to_driver(driver_id, MessageType::RideRequest(msg))
    }

    /// Driver's function
    /// Sends the AcceptRide message to the leader
    /// # Arguments
    /// * `msg` - The message containing the ride request
    fn accept_ride_request(&mut self, ride_request_msg: RideRequest) -> Result<(), io::Error> {

        // Cambiar el estado del driver a Driving
        self.state = States::Driving;

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

    /// Driver's function
    /// Simulates the travel from the origin to the destination
    /// # Arguments
    /// * `msg` - The message containing the ride request
    /// * `addr` - The address of the driver
    fn drive_and_finish(&mut self, ride_request_msg: RideRequest, addr: Addr<Self>) -> Result<(), io::Error> {
        let msg_clone = ride_request_msg.clone();
        let driver_id = self.id.clone();
        let duration = calculate_travel_duration(&msg_clone);
        let write_half = Arc::clone(&self.write_half_to_leader);

        actix::spawn(async move {
            let mut current_position = (msg_clone.x_origin, msg_clone.y_origin);
            let advancement = 10;  // Dividir el viaje en 10 pasos
            let x_km = (msg_clone.x_dest as i32 - msg_clone.x_origin as i32) as f64 / advancement as f64;
            let y_km = (msg_clone.y_dest as i32 - msg_clone.y_origin as i32) as f64 / advancement as f64;

            for i in 0..=advancement {
                current_position.0 = (msg_clone.x_origin as f64 + x_km * i as f64) as u16;
                current_position.1 = (msg_clone.y_origin as f64 + y_km * i as f64) as u16;

                let position_update = MessageType::PositionUpdate(PositionUpdate {
                    driver_id: driver_id.clone(),
                    position: (current_position.0 as i32, current_position.1 as i32),
                });

                let write_half_clone = Arc::clone(&write_half);
                let serialized = serde_json::to_string(&position_update).unwrap();
                if let Some(mut write_half) = write_half_clone.write().unwrap().as_mut() {
                    if let Err(e) = write_half
                        .write_all(format!("{}\n", serialized).as_bytes())
                        .await
                    {
                        eprintln!("Error al enviar actualizacion de posición: {:?}", e);
                    }
                } else {
                    eprintln!("No hay conexion activa para enviar la posición.");
                }

                actix_rt::time::sleep(Duration::from_secs((duration / advancement as u64) as u64)).await;
            }

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

    /// Handles the PositionUpdate message from the driver
    pub fn handle_position_update_as_leader(&mut self, msg: PositionUpdate) -> Result<(), io::Error> {
        self.drivers_last_position.insert(msg.driver_id, msg.position);
        Ok(())
    }

    /// Sends message to the payment app containing the ride price
    pub fn send_payment(&mut self, msg: RideRequest) -> Result<(), io::Error>{
        let ride_price = calculate_price(msg);
        let message = SendPayment{id: msg.id, amount: ride_price};
        self.send_message_to_payment_app(MessageType::SendPayment(message))?;
        Ok(())
    }


    /// Checks if there is a pending ride request for the passenger
    /// If there is, sends a message to the passenger to reconnect
    /// # Arguments
    /// * `passenger_id` - The id of the passenger
    pub fn verify_pending_ride_request(&self, passenger_id: u16) -> Result<(), io::Error> {
        // Verificar si hay una solicitud pendiente
        let has_pending = self.ride_manager.has_pending_ride_request(passenger_id);

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

    /// Handles the new leader message, this means that i am the new leader
    pub fn handle_be_leader_as_driver(&mut self, msg: NewLeader, addr: Addr<Driver>) -> Result<(), io::Error> {
        self.is_leader.store(true, Ordering::SeqCst);
        println!("soy liderdddddddddddddddddddddddddddddddddddddddddd? {}", self.is_leader.load(Ordering::SeqCst));
        self.state = States::Idle;
        self.leader_port = self.id;
        self.drivers_id = msg.drivers_id;
        self.drivers_id.retain(|&x| x != self.id);
        // paro de verificar los pings del lider, ya que yo soy el lider ahora
        self.stop_pings.store(true, Ordering::SeqCst);

        let drivers_id = self.drivers_id.clone();
        tokio::spawn(async move {
            Self::initialize_new_leader(drivers_id, addr).await;
        });
        // me convierto en lider, por lo que tengo que empezar a verificar los pings de los drivers
        //self.start_ping_system(addr);
        // TODO: necesito parar check_leader_alive
        Ok(())
    }

    pub fn handle_new_leader_attributes_as_leader(&mut self, msg: NewLeaderAttributes, addr: Addr<Driver>, ) -> Result<(), io::Error> {
        self.active_drivers = Arc::new(RwLock::new(msg.active_drivers));
        self.drivers_last_position = msg.drivers_last_position;
        self.payment_write_half = Arc::new(RwLock::new(msg.payment_write_half));
        self.drivers_status = Arc::new(RwLock::new(msg.drivers_status));

        addr.do_send(StreamMessage{stream: msg.payment_read_half});

        let mut active_drivers = self.active_drivers.write().unwrap();

        for (id, (read, _)) in active_drivers.iter_mut() {
            if let Some(read_half) = read.take() {
                addr.do_send(StreamMessage{stream: Some(read_half)});
            } else {
                eprintln!("No hay un stream de lectura disponible para el conductor con id {}", id);
            }
        }

        Ok(())
    }
    async fn initialize_new_leader(drivers_id: Vec<u16>, addr: Addr<Driver>) {
        let mut drivers_id = drivers_id.clone();
        let mut active_drivers: HashMap<u16, (Option<ReadHalf<TcpStream>>, Option<WriteHalf<TcpStream>>)> = HashMap::new();
        let mut drivers_last_position: HashMap<u16, (i32, i32)> = HashMap::new();
        let mut drivers_status: HashMap<u16, DriverStatus> = HashMap::new();

        // Payment app and connection
        let mut payment_write_half: Option<WriteHalf<TcpStream>> = None;
        let mut payment_read_half: Option<ReadHalf<TcpStream>> = None;

        init::init_driver(
            &mut active_drivers,
            drivers_id,
            &mut drivers_last_position,
            true,
            &mut payment_write_half,
            &mut payment_read_half,
            &mut drivers_status).await.expect("TODO: panic message");

        addr.do_send(NewLeaderAttributes {
            active_drivers,
            drivers_last_position,
            payment_write_half,
            payment_read_half,
            drivers_status,
        });
    }

    pub fn handle_new_leader_as_driver(&mut self, msg: NewLeader) -> Result<(), io::Error> {
        self.state = States::Idle;
        self.leader_port = msg.leader_id;
        self.last_ping_by_leader.do_send(UpdateLastPing { time: Instant::now() });
        Ok(())
    }

    /// ------------------------------------------- SENDING FUNCTIONS ---------------------------------------------- ///

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

    /// Sends the ride request to the driver specified by the id, only used by the leader
    /// # Arguments
    /// * `driver_id` - The id of the driver
    /// * `msg` - The message containing the ride request
    pub fn send_message_to_driver(&mut self, driver_id: u16, msg: MessageType) -> Result<(), io::Error> {
        let mut active_drivers_clone = Arc::clone(&self.active_drivers);

        actix::spawn(async move {
            let mut active_drivers = match active_drivers_clone.write() {
                Ok(guard) => guard,
                Err(e) => {
                    eprintln!("Error al obtener el lock de escritura en `active_drivers`: {:?}", e);
                    return;
                }
            };

            if let Some((_, write_half)) = active_drivers.get_mut(&driver_id) {
                let serialized = match serde_json::to_string(&msg) {
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

        let mut closest_driver = 0;
        let mut min_distance = i32::MAX;

        //let offers_and_rides = self.ride_manager.ride_and_offers.read().unwrap();


        for (driver_id, (x_driver, y_driver)) in self.drivers_last_position.iter() {

            // Si el driver que estoy viendo ya fue ofrecido el viaje, lo salteo
            if self.ride_manager.driver_has_already_been_offered_ride(message.id, *driver_id) {
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


/// ---------------------------------------------------------- LEADER ELECTION IMPLEMENTATION ---------------------------------------------------- ///

    /// Handles the dead leader message as a driver
    /// Si estoy aca es porque me di cuenta que el lider murio
    /// TODO: PROBLEMA: PUEDE QUE OTRO DRIVER TODAVIA NO SE HAYA DADO CUENTA QUE EL LIDER SE CAYO, Y SI ENVIO EL MENSAJE NADIE VA A LEER.

    pub fn handle_dead_leader_as_driver(&mut self, addr: Addr<Driver>) -> Result<(), io::Error> {

        self.state = States::LeaderLess;

        let id = self.id;
        let socket = self.socket.clone();
        let drivers_id = self.drivers_id.clone();

        let stop = Arc::new((Mutex::new(false), Condvar::new()));
        let stop_clone = stop.clone();
        let got_ack = Arc::new((Mutex::new(None), Condvar::new()));
        let leader_id =  Arc::new((Mutex::new(None), Condvar::new()));
        //let drivers_id_arc = Arc::new(RwLock::new(drivers_id));
        let drivers_id_for_leader = Arc::new(RwLock::new(Vec::new()));

        // Lanzar un hilo para el proceso de elección
        let thread_stop = stop.clone();
        let thread_leader_id = leader_id.clone();
        let drivers_id_for_leader_clone = drivers_id_for_leader.clone();

        thread::spawn(move || {
            Self::receive(id, socket, stop_clone, got_ack, leader_id, drivers_id, drivers_id_for_leader_clone);
        });

        // Esperar a que se elija un nuevo líder
        thread::spawn(move || {
            let (lock, cvar) = &*thread_leader_id;
            let mut leader_guard = lock.lock().unwrap();
            while leader_guard.is_none() {
                leader_guard = cvar.wait(leader_guard).unwrap();
            }

            // obtengo los drivers en caso de que sea lider
            let drivers_id: Vec<u16>  = drivers_id_for_leader.read().unwrap().clone();

            if let Some(new_leader) = *leader_guard {
                addr.do_send(NewLeader { leader_id: new_leader, drivers_id });

                // Detener la ejecución del proceso de elección
                let (stop_lock, stop_cvar) = &*thread_stop;
                let mut stop_guard = stop_lock.lock().unwrap();
                *stop_guard = true;
                stop_cvar.notify_all();
            }
        });

        Ok(())
    }

     fn receive(id: u16,
                socket: Arc<UdpSocket>,
                stop: Arc<(Mutex<bool>, Condvar)>,
                got_ack: Arc<(Mutex<Option<u16>>, Condvar)>,
                leader_id_cond: Arc<(Mutex<Option<u16>>, Condvar)>,
                drivers_id: Vec<u16>,
                drivers_id_for_leader: Arc<RwLock<Vec<u16>>>) {

        //tokio::time::sleep(Duration::from_secs(2)); // Provisorio para evitar colisiones iniciales
        let initial_msg = RingMessage::Election { participants: vec![id] };


        // clono y envio mensaje inicial
         let socket_clone = socket.clone();
         let id_clone = id.clone();
         let got_ack_clone = got_ack.clone();
         let msg = serde_json::to_vec(&initial_msg).unwrap();
         let drivers_id_clone = drivers_id.clone();
        thread::spawn(move || {
            Self::safe_send_next(socket_clone, msg, id_clone, got_ack_clone, drivers_id_clone);
        });

        let mut buf = [0u8; 1024];
        loop {
            let (len, addr) = match socket.recv_from(&mut buf) {
                Ok(result) => result,
                Err(e) => {
                    eprintln!("Error al recibir mensaje: {}", e);
                    break;
                }
            };

            let message: RingMessage = match serde_json::from_slice(&buf[..len]) {
                Ok(msg) => msg,
                Err(e) => {
                    eprintln!("Error al deserializar el mensaje: {}", e);
                    break;
                }
            };

            match message {
                RingMessage::ACK { id_origin } => {
                    let (lock, cvar) = &*got_ack;
                    {
                        let mut got_ack = lock.lock().unwrap_or_else(|poisoned| {
                            eprintln!("Lock poisoned, recovering: {:?}", poisoned);
                            poisoned.into_inner()
                        });
                        *got_ack = Some(id_origin);
                    }
                    cvar.notify_all();
                }
                RingMessage::Election { mut participants } => {
                    // Envio ACK
                    let ack_msg = RingMessage::ACK { id_origin: id };
                    let serialized_ack = serde_json::to_vec(&ack_msg).unwrap();
                    if let Err(e) = socket.send_to(&serialized_ack, addr) {
                        eprintln!("Error al enviar mensaje de ack: {}", e);
                    }

                    if participants.contains(&id) {
                        // Guardo los drivers que participaron de la eleccion
                        {
                            let mut drivers_id_for_leader = drivers_id_for_leader.write().unwrap();
                            *drivers_id_for_leader = participants.clone();
                        }

                        println!("ENVIANDO MENSAJE DE COORDINADOR");
                        let leader_id = *participants.iter().max().unwrap();
                        let mut participants_election = Vec::new();
                        participants_election.push(id);
                        let msg_to_send = RingMessage::Coordinator {
                            leader_id,
                            participants: participants_election,
                        };
                        let msg = serde_json::to_vec(&msg_to_send).unwrap();
                        // TODO: ver aca, en el codgio de la practica se envia para atras
                        if let Err(e) = socket.send_to(&msg, addr) {
                            eprintln!("Error al enviar mensaje de ack: {}", e);
                        }

                    } else {
                        println!("ENVIANDO MENSAJE DE ELECCCION");
                        participants.push(id);
                        let msg_to_send = RingMessage::Election { participants };

                        // clono y envio mensaje
                        let socket_clone = socket.clone();
                        let id_clone = id.clone();
                        let got_ack_clone = got_ack.clone();
                        let msg = serde_json::to_vec(&msg_to_send).unwrap();
                        let drivers_id_clone= drivers_id.clone();
                        thread::spawn(move || {
                            Self::safe_send_next(socket_clone, msg, id_clone, got_ack_clone, drivers_id_clone);
                        });
                    }
                }
                // Mensaje de coordinador: id_origen es el que primero crea el mensaje de coordinador
                RingMessage::Coordinator { leader_id, mut participants } => {
                    *leader_id_cond.0.lock().unwrap() = Some(leader_id);
                    leader_id_cond.1.notify_all();

                    // Envio ACK
                    let ack_msg = RingMessage::ACK { id_origin: id };
                    let serialized_ack = serde_json::to_vec(&ack_msg).unwrap();
                    if let Err(e) = socket.send_to(&serialized_ack, addr) {
                        eprintln!("Error al enviar mensaje de ack: {}", e);
                    }

                    if !participants.contains(&id) {
                        // sigo el anillo
                        println!("REENVIANDO MENSAJE DE COORDINADOR");
                        participants.push(id);
                        let msg_to_send = RingMessage::Coordinator { leader_id, participants };
                        let serialized_message = serde_json::to_vec(&msg_to_send).unwrap();
                        let socket_clone = socket.clone();
                        let id_clone = id.clone();
                        let got_ack_clone = got_ack.clone();
                        let drivers_id_clone = drivers_id.clone();
                        thread::spawn(move || {
                            Self::safe_send_next(socket_clone, serialized_message, id_clone, got_ack_clone, drivers_id_clone);
                        });
                    }
                }
            }
        }

    }

    pub fn safe_send_next(
        socket: Arc<UdpSocket>,
        msg: Vec<u8>,
        current_id: u16,
        got_ack: Arc<(Mutex<Option<u16>>, Condvar)>, // Para manejar el ACK
        drivers_id: Vec<u16>,
    ) {

        let mut next_id = current_id + 1; // Lógica para determinar el siguiente nodo

        // si se paso, que vuelva al primero
        if next_id == drivers_id.last().unwrap() + 1 {
            next_id = *drivers_id.first().unwrap();
        }

        // 4000 para que sean 10001, 10002 etc
        let target_address = format!("127.0.0.1:{}", next_id + 4000);

        println!("Enviando mensaje a {}", next_id);
        // Enviar el mensaje
        if let Err(e) = socket.send_to(&msg, &target_address) {
            eprintln!("Error enviando mensaje a {}: {}", next_id, e);
            return;
        }

        let got_ack_clone = got_ack.clone();
        let got_ack = got_ack.1.wait_timeout_while(got_ack.0.lock().unwrap(), TIMEOUT, |got_it| got_it.is_none() || got_it.unwrap() != next_id );
        if got_ack.unwrap().1.timed_out() {
            // Si se agotó el tiempo, reintentar con el siguiente
            println!("[{}] Timeout esperando ACK de {}", current_id, next_id);
            Self::safe_send_next(socket, msg, next_id, got_ack_clone, drivers_id);
        } else {
            println!("[{}] ACK recibido de {}", current_id, next_id);
        }
    }
}

/// Check if the driver is alive, if the driver does not respond in 5 seconds, it is considered dead
/// If dead, remove the driver from the active drivers and add it to the dead drivers
/// # Arguments
/// * `status` - The status of the driver
/// * `active_drivers` - The active drivers
/// * `dead_drivers` - The dead drivers
/// * `driver_id` - The id of the driver
/// # Returns
/// True if the driver is alive, false otherwise
/// TODO: sacar el driver de drivers_status?
pub fn update_driver_status(status: &mut DriverStatus,
                            driver_id: u16) -> bool {
    // tomo el tiempo actual
    let time_now = std::time::Instant::now();
    let mut is_alive = true;
    if let Some(last_response) = status.last_response {
        // considero muerto si no responde en 5 segundos
        if time_now.duration_since(last_response).as_secs() > PING_INTERVAL {
            status.is_alive = false;
            is_alive = false;
        }
    }
    is_alive
}