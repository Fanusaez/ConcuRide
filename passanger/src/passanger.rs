use std::hash::Hash;
use std::io;
use actix::{Actor, Context, StreamHandler, ActorFutureExt, Handler, Addr};
use actix_async_handler::async_handler;
use tokio::io::{split, AsyncBufReadExt, AsyncWriteExt, BufReader, WriteHalf};
use tokio::net::TcpStream;
use tokio::sync::oneshot::Sender;
use tokio_stream::wrappers::LinesStream;
use crate::utils::Coordinates;
const LEADER_PORT: u16 = 6000;


/// Actor that represents a passenger
pub struct Passenger {
    id: u16,
    leader_port: u16,
    sender_half: Option<WriteHalf<TcpStream>>,
    rides: Vec<Coordinates>,
}


impl Actor for Passenger {
    type Context = Context<Self>;
}

/// Handles incoming messages from the leader
impl StreamHandler<Result<String, io::Error>> for Passenger {
    fn handle(&mut self, read: Result<String, io::Error>, _ctx: &mut Self::Context) {
        if let Ok(line) = read {
            println!("{}", line);
        } else {
            println!("[{:?}] Failed to read line {:?}", self.id, read);
        }
    }
}

/// Handles Coordinates messages coming from the Passenger actor and sends them to the leader
#[async_handler]
impl Handler<Coordinates> for Passenger {
    type Result = ();

    async fn handle(&mut self, msg: Coordinates, ctx: &mut Self::Context) -> Self::Result {
        let mut write = self.sender_half.take()
            .expect("No debería poder llegar otro mensaje antes de que vuelva por usar AtomicResponse");

        let serialized = serde_json::to_string(&msg).expect("should serialize");

        let ret_write = async move {
            write
                .write_all(serialized.as_bytes()).await
                .expect("should have sent");
            write.flush().await.expect("should have flushed");
            write
        }.await;

        self.sender_half = Some(ret_write);
    }

}

impl Passenger {

    pub async fn start(port: u16, rides: Vec<Coordinates>) -> Result<(), io::Error> {
        let stream = TcpStream::connect(format!("127.0.0.1:{}", crate::LEADER_PORT)).await?;
        let rides_clone = rides.clone();
        let addr = Passenger::create(|ctx| {
            let (read, write_half) = split(stream);
            Passenger::add_stream(LinesStream::new(BufReader::new(read).lines()), ctx);
            let write = Some(write_half);
            Passenger::new(port, write, rides)
        });

        for ride in rides_clone {
            addr.try_send(ride).unwrap();
        }

        Ok(())
    }

    pub fn new(port: u16, sender_half: Option<WriteHalf<TcpStream>>, coordinates: Vec<Coordinates>) -> Self {
        Passenger {
            id: port,
            leader_port: LEADER_PORT,
            sender_half,
            rides: coordinates,
        }
    }
}
