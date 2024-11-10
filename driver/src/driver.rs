use std::collections::HashMap;
use std::io;
use std::sync::{Arc, RwLock};
use actix::{Actor, AsyncContext, Context, Handler, Message, StreamHandler};
use actix_async_handler::async_handler;
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
pub struct RideRequestResponse {
    pub id: u16,
    pub driver_id: u16,
    pub status: bool,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "message_type")]
/// enum Message used to deserialize
pub enum MessageType {
    RideRequest(RideRequest),
    RideRequestResponse(RideRequestResponse),
}

pub enum Sates {
    Driving,
    Idle,
}


const LIDER_PORT_IDX : usize = 0;

pub struct Driver {
    /// The port of the driver
    pub id: u16,
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

                MessageType::RideRequestResponse (ride_resp) => {
                    ctx.address().do_send(ride_resp);
                }
            }
        } else {
            println!("[{:?}] Failed to read line {:?}", self.id, read);
        }
    }
}

impl Handler<RideRequest> for Driver {
    type Result = ();

    fn handle(&mut self, msg: RideRequest, _ctx: &mut Self::Context) -> Self::Result {
        let is_leader = *self.is_leader.read().unwrap();
        if is_leader {
            self.handle_ride_request_as_leader(msg);
        } else {
            self.handle_ride_request(msg).expect("Error handling ride request");
        }
    }
}

impl Handler<RideRequestResponse> for Driver {
    type Result = ();

    fn handle(&mut self, msg: RideRequestResponse, _ctx: &mut Self::Context) -> Self::Result {
        println!("RECIBI RIDE REQUEST RESPONSE");
    }
}


impl Driver {
    /// Creates the actor and starts listening for incoming passengers
    /// # Arguments
    /// * `port` - The port of the driver
    /// * `drivers_ports` - The list of driver ports TODO (leader should try to connect to them)
    pub async fn start(port: u16, mut drivers_ports: Vec<u16>) -> Result<(), io::Error> {
        let should_be_leader = port == drivers_ports[LIDER_PORT_IDX];
        let is_leader = Arc::new(RwLock::new(should_be_leader));
        let leader_port = Arc::new(RwLock::new(drivers_ports[LIDER_PORT_IDX].clone()));
        let mut active_drivers: HashMap<u16, (Option<ReadHalf<TcpStream>>, Option<WriteHalf<TcpStream>>)> = HashMap::new();
        let pending_rides: Arc::<RwLock<HashMap<u16, RideRequest>>> = Arc::new(RwLock::new(HashMap::new()));
        let mut write_half: Arc<RwLock<Option<WriteHalf<TcpStream>>>> = Arc::new(RwLock::new(None));

        // Remove the leader port from the list of drivers
        drivers_ports.remove(LIDER_PORT_IDX);

        init::init_driver(&mut active_drivers, drivers_ports, should_be_leader).await?;

        let active_drivers_arc = Arc::new(RwLock::new(active_drivers));

        let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).await?;
        println!("WAITING FOR PASSENGERS TO CONNECT(leader) OR ACCEPTING LEADER(drivers)\n");

        while let Ok((stream,  _)) = listener.accept().await {

            println!("CONNECTION ACCEPTED\n");

            Driver::create(|ctx| {
                let (read_passenger, _write_half) = split(stream);
                Driver::add_stream(LinesStream::new(BufReader::new(read_passenger).lines()), ctx);

                /// Write half sera conexion hacia el Passenger si es Leader, de lo contrario sera hacia el Leader (dado que soy un driver)
                write_half.write().unwrap().replace(_write_half);

                /// asocio todos los reads de los drivers al lider
                if should_be_leader {
                    let mut active_drivers = active_drivers_arc.write().unwrap();

                    for (id, (read, _)) in active_drivers.iter_mut() {
                        if let Some(read_half) = read.take() {
                            Driver::add_stream(LinesStream::new(BufReader::new(read_half).lines()), ctx);
                        } else {
                            eprintln!("Driver {} no tiene un stream de lectura disponible", id);
                        }
                    }
                }
                Driver {
                    id: port,
                    is_leader: is_leader.clone(),
                    leader_port: leader_port.clone(),
                    active_drivers: active_drivers_arc.clone(),
                    state: Sates::Idle,
                    pending_rides: pending_rides.clone(),
                    write_half: write_half.clone(),
                }
            });
        }
        Ok(())
    }

    /// Handles the ride request from the leader as a driver
    /// # Arguments
    /// * `msg` - The message containing the ride request
    pub fn handle_ride_request(&self, msg: RideRequest) -> Result<(), io::Error> {
        let probability = 0.9;
        let result = boolean_with_probability(probability);

        if result {
            println!("Driver {} accepted the ride request", self.id);
            self.accept_ride_request(msg);
        } else {
            println!("Driver {} rejected the ride request", self.id);
        }
        Ok(())

    }

    /// Handles the ride request from passanger
    /// TODO: LOGICA PARA VER A QUIEN SE LE DAN LOS VIAJES, ACA SE ESTA MANDANDO A TODOS
    /// # Arguments
    /// * `msg` - The message containing the ride request
    pub fn handle_ride_request_as_leader(&mut self, msg: RideRequest) {

        let active_drivers_clone = Arc::clone(&self.active_drivers);
        let msg_clone = msg.clone();

        // Lo pongo el pending_rides hasta que alguien acepte el viaje
        let mut pending_rides = self.pending_rides.write().unwrap();
        pending_rides.insert(msg.id.clone(), msg.clone());

        // TODO: VER SI SE PUEDE MODULARIZAR ESTO DE ALGUNA MANERA
        // Otro lugar que se encarge de los mensajes?
        actix::spawn(async move {
            if let Ok(mut active_drivers_clone) = active_drivers_clone.write() {
                for (id, (_, write)) in active_drivers_clone.iter_mut() {
                    let mut half_write = write.take()
                        .expect("No debería poder llegar otro mensaje antes de que vuelva por usar AtomicResponse");

                    let msg_type = MessageType::RideRequest(msg_clone);
                    let serialized = serde_json::to_string(&msg_type).expect("should serialize");
                    let ret_write = async move {
                        half_write
                            .write_all(format!("{}\n", serialized).as_bytes()).await
                            .expect("should have sent");
                        half_write
                    }.await;

                    *write = Some(ret_write);
                }
            } else {
                eprintln!("No se pudo obtener el lock de lectura en `active_drivers`");
            }
        });
    }

    /// Accepts the ride request and sends the response to the leader
    /// # Arguments
    /// * `msg` - The message containing the ride request
    fn accept_ride_request(&self, msg: RideRequest) {
        // Clonar el Arc<RwLock<Option<WriteHalf<TcpStream>>>>
        let write_half = Arc::clone(&self.write_half);

        // Crear el mensaje de respuesta
        let response = RideRequestResponse {
            id: msg.id,
            driver_id: self.id,
            status: true,
        };

        // Serializar el mensaje en JSON
        let msg_type = MessageType::RideRequestResponse(response);
        let serialized = match serde_json::to_string(&msg_type) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error serializando el mensaje: {:?}", e);
                return;
            }
        };

        // Enviar el mensaje de manera asíncrona
        actix::spawn(async move {
            let mut write_guard = write_half.write().unwrap();

            if let Some(write_half) = write_guard.as_mut() {
                if let Err(e) = write_half.write_all(format!("{}\n", serialized).as_bytes()).await {
                    eprintln!("Error al enviar el mensaje: {:?}", e);
                }
            } else {
                eprintln!("No se pudo enviar el mensaje: el líder no tiene una conexión activa");
            }
        });
    }
}



pub fn boolean_with_probability(probability: f64) -> bool {
    let mut rng = rand::thread_rng();
    rng.gen::<f64>() < probability
}