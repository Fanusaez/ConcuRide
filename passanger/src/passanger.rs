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


pub struct Passenger {
    /// The port of the passenger
    id: u16,
    /// The port of the leader (6000 for now)
    leader_port: u16,
    /// The actor that sends messages to the leader
    tcp_sender: Addr<TcpSender>,
    /// The list of rides (coordinates) that the passenger has to go to
    rides: Vec<Coordinates>,
    /// The channel to send a completion signal to the main function
    completion_signal: Option<Sender<()>>,
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

/// Handles Coordinates messages coming from main and sends them to the TcpSender actor
impl Handler<Coordinates> for Passenger {
    type Result = ();

    fn handle(&mut self, msg: Coordinates, _ctx: &mut Self::Context) -> Self::Result {
        println!("Mensaje recibido por el Passenger: {:?}", msg);
        self.tcp_sender.try_send(msg).unwrap()
    }
}

impl Passenger {
    /// Creates the actor and connects to the leader
    /// # Arguments
    /// * `port` - The port of the passenger
    /// * `rides` - The list of rides (coordinates) that the passenger has to go to
    /// * `tx` - The channel to send a completion signal to the main function
    pub async fn start(port: u16, rides: Vec<Coordinates>, tx: Sender<()>) -> Result<(), io::Error> {
        let stream = TcpStream::connect(format!("127.0.0.1:{}", crate::LEADER_PORT)).await?;
        let rides_clone = rides.clone();
        let addr = Passenger::create(|ctx| {
            let (read, write_half) = split(stream);
            Passenger::add_stream(LinesStream::new(BufReader::new(read).lines()), ctx);
            let write = Some(write_half);
            let addr_tcp = TcpSender::new(write).start();
            Passenger::new(port, addr_tcp, rides, tx)
        });

        // Send the rides to myself to be processed
        for ride in rides_clone {
            addr.send(ride).await.unwrap();
        }

        Ok(())
    }

    pub fn new(port: u16, tcp_sender: Addr<TcpSender>, coordinates: Vec<Coordinates>, tx: Sender<()>) -> Self {
        Passenger {
            id: port,
            leader_port: LEADER_PORT,
            tcp_sender,
            rides: coordinates,
            completion_signal: Some(tx),
        }
    }
}


pub struct TcpSender {
    /// The write half of the TcpStream
    write: Option<WriteHalf<TcpStream>>,
}

/// The actor that sends messages to the leader
impl Actor for TcpSender {
    type Context = Context<Self>;
}

impl TcpSender {
    pub fn new(write: Option<WriteHalf<TcpStream>>) -> Self {
        TcpSender { write }
    }
}

/// Handles Coordinates messages coming from the Passenger actor and sends them to the leader
#[async_handler]
impl Handler<Coordinates> for TcpSender {
    type Result = ();

    async fn handle(&mut self, msg: Coordinates, ctx: &mut Self::Context) -> Self::Result {
        let mut write = self.write.take()
            .expect("No debería poder llegar otro mensaje antes de que vuelva por usar AtomicResponse");

        //let serialized = serde_json::to_string(&msg).expect("should serialize");
        let serialized = format!("{:?}\n", msg);
        let ret_write = async move {
            write
                .write_all(serialized.as_bytes()).await
                .expect("should have sent");
            write
        }.await;

        self.write = Some(ret_write);
    }

}
