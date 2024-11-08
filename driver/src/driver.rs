use std::collections::HashMap;
use std::io;
use std::sync::{Arc, RwLock};
use actix::{Actor, AsyncContext, Context, Handler, Message, StreamHandler};
use tokio::io::{split, AsyncBufReadExt, BufReader, AsyncWriteExt, WriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio_stream::wrappers::LinesStream;
use serde::{Serialize, Deserialize};

/// Coordinates struct, ver como se puede importar desde otro archivo, esto esta en utils.rs\
#[derive(Serialize, Deserialize, Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct Coordinates {
    pub x_origin: u16,
    pub y_origin: u16,
    pub x_dest: u16,
    pub y_dest: u16,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "message_type")]
/// enum Message used to deserialize
pub enum MessageType {
    Coordinates(Coordinates),
    // TODO: Add more message types, StatusUpdate is useless for now.
    StatusUpdate { status: String },
}


const LIDER_PORT_IDX : usize = 0;

pub struct Driver {
    /// The port of the driver
    pub id: u16,
    /// Whether the driver is the leader
    pub is_leader: Arc<RwLock<bool>>,
    // The connections to the drivers TODO
    pub active_drivers: Arc<HashMap<u16, WriteHalf<TcpStream>>>,
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
                MessageType::Coordinates(coords)=> {
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


impl Handler<Coordinates> for Driver {
    type Result = ();

    fn handle(&mut self, msg: Coordinates, _ctx: &mut Self::Context) -> Self::Result {
        println!("Received coordinates from passenger: {:?}", msg);
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
        let mut active_drivers: HashMap<u16, WriteHalf<TcpStream>> = HashMap::new();

        // Remove the leader port from the list of drivers
        drivers_ports.remove(LIDER_PORT_IDX);


        if *is_leader.read().unwrap() {
            /// Connect to the other drivers and save connections
            /// TODO: Habria que modularizar esto y moverlo a un diferente archivo
            for driver_port in drivers_ports.iter() {
                let stream = TcpStream::connect(format!("127.0.0.1:{}", driver_port)).await?;
                let (_, write_half) = split(stream);
                active_drivers.insert(*driver_port, write_half);
            }
        }

        else {
        }

        let active_drivers_arc = Arc::new(active_drivers);

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
                }
            });
        }
        Ok(())
    }
}