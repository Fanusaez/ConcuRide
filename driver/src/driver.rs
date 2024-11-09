use std::collections::HashMap;
use std::io;
use std::sync::{Arc, RwLock};
use actix::{Actor, AsyncContext, Context, Handler, Message, StreamHandler};
use actix_async_handler::async_handler;
use tokio::io::{split, AsyncBufReadExt, BufReader, AsyncWriteExt, WriteHalf, AsyncReadExt};
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

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "message_type")]
/// enum Message used to deserialize
pub enum MessageType {
    RideRequest(RideRequest),
    // TODO: Add more message types, StatusUpdate is useless for now.
    StatusUpdate { status: String },
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
    /// The connections to the drivers
    pub active_drivers: Arc<RwLock<HashMap<u16, Option<WriteHalf<TcpStream>>>>>,
    /// States of the driver
    pub state: Sates,
    /// Pending rides
    pub pending_rides: Arc<RwLock<HashMap<u16, RideRequest>>>,
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

                MessageType::StatusUpdate { status } => {
                    println!("Status: {}", status);
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
            self.handle_ride_request_as_lider(msg);
        } else {
            self.handle_ride_request(msg);
        }
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
        let mut active_drivers: HashMap<u16, Option<WriteHalf<TcpStream>>> = HashMap::new();
        let pending_rides: Arc::<RwLock<HashMap<u16, RideRequest>>> = Arc::new(RwLock::new(HashMap::new()));

        // Remove the leader port from the list of drivers
        drivers_ports.remove(LIDER_PORT_IDX);

        init::init_driver(&mut active_drivers, drivers_ports, should_be_leader).await?;

        let active_drivers_arc = Arc::new(RwLock::new(active_drivers));

        let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).await?;
        println!("WAITING FOR PASSENGERS TO CONNECT(leader) OR ACCEPTING LEADER(drivers)\n");

        while let Ok((stream,  _)) = listener.accept().await {

            println!("CONNECTION ACCEPTED\n");

             Driver::create(|ctx| {
                let (read, _write_half) = split(stream);
                Driver::add_stream(LinesStream::new(BufReader::new(read).lines()), ctx);
                //let write = Some(write_half);
                Driver {
                    id: port,
                    is_leader: is_leader.clone(),
                    active_drivers: active_drivers_arc.clone(),
                    state: Sates::Idle,
                    pending_rides: pending_rides.clone(),
                }
            });
        }
        Ok(())
    }

    /// Handles the ride request from the leader
    pub fn handle_ride_request(&self, msg: RideRequest) {
        /// TODO
        println!("Ride request received by diver 6001: {:?}", msg);
    }

    pub fn handle_ride_request_as_lider(&self, msg: RideRequest) {

        let active_drivers_clone = Arc::clone(&self.active_drivers);
        let msg_clone = msg.clone();

        // TODO: VER SI SE PUEDE MODULARIZAR ESTO DE ALGUNA MANERA
        // Otro lugar que se encarge de los mensajes?
        actix::spawn(async move {
            if let Ok(mut active_drivers_clone) = active_drivers_clone.write() {
                for (id, write) in active_drivers_clone.iter_mut() {
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
}