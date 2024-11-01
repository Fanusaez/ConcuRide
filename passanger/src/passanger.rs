use std::collections::HashMap;
use std::hash::Hash;
use std::io;
use actix::{Actor, Context, StreamHandler};
use tokio::io::{WriteHalf};
use crate::utils::Coordinates;
const LEADER_PORT: u16 = 6000;

pub struct Passanger {
    id: u16,
    leader_port: u16,
    rides: HashMap<u16, Coordinates>,
    write: Option<WriteHalf<tokio::net::TcpStream>>,
}

impl Actor for Passanger {
    type Context = Context<Self>;

    // Mét odo `started` se llama automáticamente cuando el actor es iniciado
    fn started(&mut self, ctx: &mut Self::Context) {
        self.start();
    }
}

/// Handles incoming messages from the leader
impl StreamHandler<Result<String, std::io::Error>> for Passanger {
    fn handle(&mut self, read: Result<String, io::Error>, _ctx: &mut Self::Context) {

        if let Ok(line) = read {
            println!("{}", line);
        } else {
            println!("[{:?}] Failed to read line {:?}", self.id, read);
        }
    }
}

impl Passanger {
    pub fn new(port: u16, rides: HashMap<u16, Coordinates>, write_half: Option<WriteHalf<tokio::net::TcpStream>>) -> Self {
        Passanger {
            id: port,
            leader_port: LEADER_PORT,
            rides,
            write: write_half,
        }
    }

    /// Starts reading the rides and sending them to the leader
    /// Esta seria como el main, itera sobre los rides y los va enviando al lider
    pub fn start(&mut self) {
        // lopp through orders and send them to the leader
        for (id, coord) in &self.rides {
            let msg = format!("{} {} {} {} {}", id, coord.x_origin, coord.y_origin, coord.x_dest, coord.y_dest);
            print!("Sending: {}", msg);
        }
    }
}
