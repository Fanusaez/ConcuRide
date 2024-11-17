use std::cmp::PartialEq;
use std::collections::HashMap;
use std::{io, thread};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use actix::{Actor, Addr, AsyncContext, Context, Handler, Message, StreamHandler};
use rand::Rng;
use tokio::io::{split, AsyncBufReadExt, BufReader, AsyncWriteExt, WriteHalf, AsyncReadExt, ReadHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio_stream::wrappers::LinesStream;

use crate::init;
use crate::models::*;

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
    /// The connections to the drivers
    pub active_drivers: Arc<RwLock<HashMap<u16, (Option<ReadHalf<TcpStream>>, Option<WriteHalf<TcpStream>>)>>>,
    /// States of the driver
    pub state: Sates,
    /// Pending rides, already paid rides, waiting to be accepted by a driver
    pub pending_rides: Arc<RwLock<HashMap<u16, RideRequest>>>,
    /// Connection to the leader or the Passenger
    pub write_half: Arc<RwLock<Option<WriteHalf<TcpStream>>>>,
    /// Last known position of the driver (port, (x, y))
    pub drivers_last_position: Arc::<RwLock<HashMap<u16, (i32, i32)>>>,
    /// Passenger last DriveRequest and the drivers who have been offered the ride
    /// Veremos si es necesario, sino lo podemos volar
    pub ride_and_offers: Arc::<RwLock<HashMap<u16, Vec<u16>>>>,
    /// Connection to the payment app
    pub payment_write_half: Arc<RwLock<Option<WriteHalf<TcpStream>>>>,
    /// Already paid rides (ready to send to drivers)
    pub unpaid_rides: Arc<RwLock<HashMap<u16, RideRequest>>>,
}

impl Driver {
    /// Creates the actor and starts listening for incoming passengers
    /// # Arguments
    /// * `port` - The port of the driver
    /// * `drivers_ports` - The list of driver ports TODO (leader should try to connect to them)
    pub async fn start(port: u16, mut drivers_ports: Vec<u16>) -> Result<(), io::Error> {
        // Driver/leader attributes
        let should_be_leader = port == drivers_ports[LIDER_PORT_IDX];
        let is_leader = Arc::new(RwLock::new(should_be_leader));
        let leader_port = Arc::new(RwLock::new(drivers_ports[LIDER_PORT_IDX].clone()));
        let mut write_half: Arc<RwLock<Option<WriteHalf<TcpStream>>>> = Arc::new(RwLock::new(None));

        // Auxiliar structures
        let mut active_drivers: HashMap<u16, (Option<ReadHalf<TcpStream>>, Option<WriteHalf<TcpStream>>)> = HashMap::new();
        let mut drivers_last_position: HashMap<u16, (i32, i32)> = HashMap::new();
        let pending_rides: Arc<RwLock<HashMap<u16, RideRequest>>> = Arc::new(RwLock::new(HashMap::new()));
        let ride_and_offers: Arc::<RwLock<HashMap<u16, Vec<u16>>>> = Arc::new(RwLock::new(HashMap::new()));
        let mut streams_added = false;

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

        let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).await?;
        println!("WAITING FOR PASSENGERS TO CONNECT(leader) OR ACCEPTING LEADER(drivers)\n");

        while let Ok((stream,  _)) = listener.accept().await {

            println!("CONNECTION ACCEPTED\n");

            Driver::create(|ctx| {
                let (read_half, _write_half) = split(stream);
                Driver::add_stream(LinesStream::new(BufReader::new(read_half).lines()), ctx);

                /// Write half sera conexion hacia el Passenger si es Leader, de lo contrario sera hacia el Leader (dado que soy un driver)
                write_half.write().unwrap().replace(_write_half);

                /// asocio todos los reads de los drivers al lider
                if should_be_leader && !streams_added {
                    let mut active_drivers = active_drivers_arc.write().unwrap();

                    for (id, (read, _)) in active_drivers.iter_mut() {
                        if let Some(read_half) = read.take() {
                            Driver::add_stream(LinesStream::new(BufReader::new(read_half).lines()), ctx);
                        } else {
                            eprintln!("Driver {} no tiene un stream de lectura disponible", id);
                        }
                    }

                    /// asocio el read del servicio de pagos al lider
                    let mut payment_read_half = payment_read_half_arc.write().unwrap();
                    if let Some(payment_read_half) = payment_read_half.take() {
                        Driver::add_stream(LinesStream::new(BufReader::new(payment_read_half).lines()), ctx);
                    } else {
                        eprintln!("No hay un stream de lectura disponible para el servicio de pagos");
                    }
                    streams_added = true;
                }
                Driver {
                    id: port,
                    position: (0, 0), // Arrancan en el origen por comodidad, ver despues que onda
                    is_leader: is_leader.clone(),
                    leader_port: leader_port.clone(),
                    active_drivers: active_drivers_arc.clone(),
                    state: Sates::Idle,
                    pending_rides: pending_rides.clone(),
                    write_half: write_half.clone(),
                    drivers_last_position: drivers_last_position_arc.clone(),
                    ride_and_offers: ride_and_offers.clone(),
                    payment_write_half: payment_write_half_arc.clone(),
                    unpaid_rides: unpaid_rides.clone(),

                }
            });
        }
        Ok(())
    }

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

    /// Handles the ride request from passanger, sends RideRequest to the closest driver
    /// TODO: LOGICA PARA VER A QUIEN SE LE DAN LOS VIAJES, ACA SE ESTA MANDANDO A TODOS
    /// # Arguments
    /// * `msg` - The message containing the ride request
    pub fn handle_ride_request_as_leader(&mut self, msg: RideRequest) -> Result<(), io::Error>{

        /// saves ride in pending_rides
        self.insert_ride_in_pending(msg)?;

        /// Logica de a quien se le manda el mensaje
        let driver_id_to_send = self.get_closest_driver(msg);

        /// Agrego el id del driver al vector de ofertas
        self.insert_in_rides_and_offers(msg.id, driver_id_to_send)?;


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

            if let Some((_, write_half)) = active_drivers.get_mut(&driver_id_to_send) {
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
        self.send_message(accept_msg)?;

        Ok(())
    }

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
        self.send_message(msg_type)?;

        Ok(())
    }

    /// Generic function to send a message to the leader or the passenger
    /// This function use de write stream associated when actor created
    fn send_message(&self, message: MessageType) -> Result<(), io::Error> {
        let write_half = Arc::clone(&self.write_half);

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

    /// Returns the id to the closest driver to the passenger
    /// # Arguments
    /// * `message` - The message containing the ride request
    /// # Returns
    /// The id of the closest driver
    fn get_closest_driver(&self, message: RideRequest) -> u16 {
        // PickUp position
        let (x_passenger, y_passenger) = (message.x_origin as i32, message.y_origin as i32);

        let drivers_last_position = self.drivers_last_position.read().unwrap();
        let mut closest_driver = 0;
        let mut min_distance = i32::MAX;


        for (driver_id, (x_driver, y_driver)) in drivers_last_position.iter() {
            let distance = (x_passenger - x_driver).abs() + (y_passenger - y_driver).abs();
            if distance < min_distance {
                min_distance = distance;
                closest_driver = *driver_id;
            }
        }
        closest_driver
    }

    /// Inserts a ride in the pending rides
    /// # Arguments
    /// * `msg` - The message containing the ride request
    fn insert_ride_in_pending(&self, msg: RideRequest) -> Result<(), io::Error> {
        // Lo pongo el pending_rides hasta que alguien acepte el viaje
        let mut pending_rides = self.pending_rides.write();
        match pending_rides {
            Ok(mut pending_rides) => {
                pending_rides.insert(msg.id.clone(), msg.clone());
            },
            Err(e) => {
                eprintln!("Error al obtener el lock de escritura en `pending_rides`: {:?}", e);
                return Err(io::Error::new(io::ErrorKind::Other, "Error al obtener el lock de escritura en `pending_rides`"));
            }
        }
        Ok(())
    }

    /// Inserts the passenger id and the driver id in the ride_and_offers hashmap
    /// # Arguments
    /// * `passenger_id` - The id of the passenger
    /// * `driver_id` - The id of the driver
    fn insert_in_rides_and_offers(&self, passenger_id: u16, driver_id: u16) -> Result<(), io::Error> {
        let mut ride_and_offers = self.ride_and_offers.write();
        match ride_and_offers {
            Ok(mut ride_and_offers) => {
                if let Some(offers) = ride_and_offers.get_mut(&passenger_id) {
                    offers.push(driver_id);
                } else {
                    ride_and_offers.insert(passenger_id, vec![driver_id]);
                }
            },
            Err(e) => {
                eprintln!("Error al obtener el lock de escritura en `ride_and_offers`: {:?}", e);
                return Err(io::Error::new(io::ErrorKind::Other, "Error al obtener el lock de escritura en `ride_and_offers`"));
            }
        }
        Ok(())
    }

    /// Drive to the destination and finish the ride
    /// # Arguments
    /// * `msg` - The message containing the ride request
    /// * `addr` - The address of the driver
    fn drive_and_finish(&self, ride_request_msg: RideRequest, addr: Addr<Self>) -> Result<(), io::Error> {

        let msg_clone = ride_request_msg.clone();
        let driver_id = self.id.clone();

        thread::spawn(move || {
            let distance = (msg_clone.x_dest as i32 - msg_clone.x_origin as i32).abs()
                + (msg_clone.y_dest as i32 - msg_clone.y_origin as i32).abs();
            thread::sleep(Duration::from_secs(distance as u64));

            let finish_ride = FinishRide {
                passenger_id: ride_request_msg.id,
                driver_id,
            };

            addr.do_send(finish_ride);
        });

        Ok(())
    }

    /// Finish the ride, send the FinishRide message to the leader
    /// # Arguments
    /// * `msg` - The message containing the ride request
    pub fn finish_ride(&mut self, msg: FinishRide) -> Result<(), io::Error> {
        self.state = Sates::Idle;
        self.send_message(MessageType::FinishRide(msg))?;
        Ok(())
    }

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

    /// Upon receiving a ride request, the leader will add it to the unpaid rides
    /// # Arguments
    /// * `msg` - The message containing the ride request
    pub fn insert_unpaid_ride(&self, msg: RideRequest) -> Result<(), io::Error> {
        let mut unpaid_rides = self.unpaid_rides.write();
        match unpaid_rides {
            Ok(mut unpaid_rides) => {
                unpaid_rides.insert(msg.id.clone(), msg.clone());
            },
            Err(e) => {
                eprintln!("Error al obtener el lock de escritura en `unpaid_rides`: {:?}", e);
                return Err(io::Error::new(io::ErrorKind::Other, "Error al obtener el lock de escritura en `unpaid_rides`"));
            }
        }
        Ok(())
    }

    pub fn remove_ride_from_pending(&self, passenger_id: u16) {
        let mut pending_rides = self.pending_rides.write().unwrap();
        if (pending_rides.remove(&passenger_id)).is_none() {
            eprintln!("RideRequest with id {} not found in pending_rides", passenger_id);
        }
    }

    pub fn remove_ride_from_unpaid(&self, passenger_id: u16) {
        let mut unpaid_rides = self.unpaid_rides.write().unwrap();
        if (unpaid_rides.remove(&passenger_id)).is_none() {
            eprintln!("RideRequest with id {} not found in unpaid_rides", passenger_id);
        }
    }

}

pub fn boolean_with_probability(probability: f64) -> bool {
    let mut rng = rand::thread_rng();
    rng.gen::<f64>() < probability
}