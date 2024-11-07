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
    tcp_sender: Addr<TcpSender>,
    rides: Vec<Coordinates>,
    completion_signal: Option<Sender<()>>,
}

/// Actor that sends messages to the leader
pub struct TcpSender {
    write: Option<WriteHalf<tokio::net::TcpStream>>,
}

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

        // Aca puede estar el problema
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
