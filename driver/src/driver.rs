use std::cmp::PartialEq;
use std::collections::HashMap;
use std::{io, thread};
use std::sync::{mpsc, Arc, RwLock};
use std::time::Duration;
use actix::{Actor, Addr, AsyncContext, Context, Handler, Message, StreamHandler};
use actix_async_handler::async_handler;
use futures::future::MaybeDone::Future;
use rand::Rng;
use tokio::io::{split, AsyncBufReadExt, BufReader, AsyncWriteExt, WriteHalf, AsyncReadExt, ReadHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio_stream::wrappers::LinesStream;
use serde::{Serialize, Deserialize};
use crate::init;


/// RideRequest struct, ver como se puede importar desde otro archivo, esto esta en utils.rs\
#[derive(Serialize, Deserialize, Message, Debug, Clone, Copy)]
#[rtype(result = "()")]
pub struct RideRequest {
    pub id: u16,
    pub x_origin: u16,
    pub y_origin: u16,
    pub x_dest: u16,
    pub y_dest: u16,
}

#[derive(Serialize, Deserialize, Message, Debug, Clone, Copy)]
#[rtype(result = "()")]
pub struct AcceptRide {
    pub passenger_id: u16,
    pub driver_id: u16,
}

#[derive(Serialize, Deserialize, Message, Debug, Clone, Copy)]
#[rtype(result = "()")]
pub struct DeclineRide {
    pub passenger_id: u16,
    pub driver_id: u16,
}

#[derive(Serialize, Deserialize, Message, Debug, Clone, Copy)]
#[rtype(result = "()")]
pub struct FinishRide {
    pub passenger_id: u16,
    pub driver_id: u16,
}

#[derive(Serialize, Deserialize, Message, Debug, Clone, Copy)]
#[rtype(result = "()")]
pub struct SendPayment {
    pub id: u16,
    pub amount: i32,
}

#[derive(Serialize, Deserialize, Message, Debug, Clone, Copy)]
#[rtype(result = "()")]
pub struct PaymentRejected {
    pub id: u16,
}

#[derive(Serialize, Deserialize, Message, Debug, Clone, Copy)]
#[rtype(result = "()")]
pub struct PaymentAccepted {
    pub id: u16,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "message_type")]
/// enum Message used to deserialize
pub enum MessageType {
    RideRequest(RideRequest),
    AcceptRide(AcceptRide),
    DeclineRide(DeclineRide),
    FinishRide(FinishRide),
    SendPayment(SendPayment),
    PaymentAccepted(PaymentAccepted),
    PaymentRejected(PaymentRejected),
}

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
    /// Pending rides
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
    pub paid_rides: Arc<RwLock<HashMap<u16, RideRequest>>>,
}

impl Actor for Driver {
    type Context = Context<Self>;
}

impl StreamHandler<Result<String, io::Error>> for Driver {
    /// Handles the messages coming from the associated stream.
    /// Matches the message type and sends it to the corresponding handler.
    fn handle(&mut self, read: Result<String, io::Error>, ctx: &mut Self::Context) {
        if let Ok(line) = read {
            let message: MessageType = serde_json::from_str(&line).expect("Failed to deserialize message");
            match message {
                MessageType::RideRequest(coords)=> {
                    ctx.address().do_send(coords);
                }

                MessageType::AcceptRide (accept_ride) => {
                    ctx.address().do_send(accept_ride);
                }

                MessageType::DeclineRide (decline_ride) => {
                    ctx.address().do_send(decline_ride);
                }

                MessageType::FinishRide (finish_ride) => {
                    ctx.address().do_send(finish_ride);
                }

                MessageType::PaymentAccepted(payment_accepted) => {
                    ctx.address().do_send(payment_accepted);
                }

                MessageType::PaymentRejected(payment_rejected) => {
                    ctx.address().do_send(payment_rejected);
                }
                _ => {
                    println!("Unknown Message");
                }
            }
        } else {
            println!("[{:?}] Failed to read line {:?}", self.id, read);
        }
    }
}

impl Handler<RideRequest> for Driver {
    type Result = ();

    /// Handles the ride request message depending on whether the driver is the leader or not.
    fn handle(&mut self, msg: RideRequest, ctx: &mut Self::Context) -> Self::Result {
        let is_leader = *self.is_leader.read().unwrap();
        
        if is_leader {
            println!("Making reservation for payment of ride request from passenger {}", msg.id);
            self.send_payment(msg).expect("Error sending payment");
        } else {
            self.handle_ride_request_as_driver(msg, ctx.address()).expect("Error handling ride request as driver");
        }
    }
}


impl Handler<PaymentAccepted> for Driver {
    type Result = ();

    /// Only receved by leader
    /// Handles the payment accepted message
    fn handle(&mut self, msg: PaymentAccepted, ctx: &mut Self::Context) -> Self::Result {

        println!("Leader {} received the payment accepted message for the ride request from passenger {}", self.id, msg.id);

        /// Hay que remover antes de terminar?
        let ride_request = {
            let mut pending_rides = self.pending_rides.write().unwrap();
            pending_rides.remove(&msg.id)
        };

        //TODO: hay que ver el tema de los ids de los viajes (No se deberian repetir?)
        match ride_request {
            Some(ride_request) => {
               self.handle_ride_request_as_leader(ride_request).unwrap()
            }
            None => {
                eprintln!("RideRequest with id {} not found in pending_rides", msg.id);
            }
        }
    }
}

impl Handler<PaymentRejected> for Driver {
    type Result = ();

    fn handle(&mut self, msg: PaymentRejected, ctx: &mut Self::Context) -> Self::Result {
        // TODO: avisar al cliente que se rechazo el pago de viaje
    }

}


impl Handler<AcceptRide> for Driver {
    type Result = ();
    /// Only received by leader
    fn handle(&mut self, msg: AcceptRide, _ctx: &mut Self::Context) -> Self::Result {
        /// TODO: Sacar de ride_and_offers le id del pasajero y el driver que acepto dado que ya se acepto
        /// TODO: Pending_rides se saca una vez que notifico al pasajero
        println!("Lider {} received the accept response for the ride request from driver {}", self.id, msg.driver_id);
    }
}

impl Handler<DeclineRide> for Driver {
    type Result = ();
    /// Only received by leader
    fn handle(&mut self, msg: DeclineRide, _ctx: &mut Self::Context) -> Self::Result {
        println!("Lider {} received the declined message for the ride request from driver {}", self.id, msg.driver_id);
        // TODO: volver a elegir a quien ofrecer el viaje
    }
}

impl Handler<FinishRide> for Driver {
    type Result = ();

    fn handle(&mut self, msg: FinishRide, _ctx: &mut Self::Context) -> Self::Result {
        let is_leader = *self.is_leader.read().unwrap();
        if is_leader {
            println!("Lider {} received the finish ride message from driver {}", self.id, msg.driver_id);
            // TODO: Eliminar de pending rides y avisar al pasajero.
        } else {
            // driver send FinishRide to the leader and change state to Idle
            self.finish_ride(msg).unwrap()
        }
    }
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
        let paid_rides: Arc<RwLock<HashMap<u16, RideRequest>>>= Arc::new(RwLock::new(HashMap::new()));

        // Remove the leader port from the list of drivers
        drivers_ports.remove(LIDER_PORT_IDX);

        init::init_driver(&mut active_drivers, drivers_ports, &mut drivers_last_position, should_be_leader, &mut payment_write_half).await?;

        // Arcs for shared data
        let mut active_drivers_arc = Arc::new(RwLock::new(active_drivers));
        let mut drivers_last_position_arc = Arc::new(RwLock::new(drivers_last_position));
        let mut payment_write_half_arc = Arc::new(RwLock::new(payment_write_half));

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
                    paid_rides: paid_rides.clone(),

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
    fn finish_ride(&mut self, msg: FinishRide) -> Result<(), io::Error> {
        self.state = Sates::Idle;
        self.send_message(MessageType::FinishRide(msg))?;
        Ok(())
    }

    fn send_payment(&mut self, msg: RideRequest) -> Result<(), io::Error>{
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
}

pub fn boolean_with_probability(probability: f64) -> bool {
    let mut rng = rand::thread_rng();
    rng.gen::<f64>() < probability
}