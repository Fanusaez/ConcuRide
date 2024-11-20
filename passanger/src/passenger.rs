use std::cmp::PartialEq;
use std::hash::Hash;
use std::io;
use actix::{Actor, Context, StreamHandler, ActorFutureExt, Handler, Addr, AsyncContext};
use actix_async_handler::async_handler;
use tokio::io::{split, AsyncBufReadExt, AsyncWriteExt, BufReader, WriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio_stream::wrappers::LinesStream;
use tokio::net::TcpSocket;

use crate::models::*;

const LEADER_PORT: u16 = 6000;


pub enum Sates {
    /// Before requesting a ride or after drop off
    Idle,
    /// After requesting a ride and paying, waiting for a driver to accept the ride
    WaitingDriver,
    /// Riding in a car, necesario????
    Traveling,
}

impl PartialEq for Sates {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Sates::Idle, Sates::Idle) => true,
            (Sates::WaitingDriver, Sates::WaitingDriver) => true,
            (Sates::Traveling, Sates::Traveling) => true,
            _ => false,
        }
    }
}

pub struct Passenger {
    /// The port of the passenger
    id: u16,
    /// The port of the leader (6000 for now)
    leader_port: u16,
    /// The actor that sends messages to the leader
    tcp_sender: Addr<TcpSender>,
    /// State of the passenger
    state: Sates,
    /// Next Drive Request to be processed
    rides: Vec<RideRequest>,
}

impl Actor for Passenger {
    type Context = Context<Self>;
}

/// Handles incoming messages from the leader
impl StreamHandler<Result<String, io::Error>> for Passenger {

    fn handle(&mut self, read: Result<String, io::Error>, ctx: &mut Self::Context) {
        if let Ok(line) = read {
            let message: MessageType = serde_json::from_str(&line).expect("Failed to deserialize message");
            match message {
                MessageType::FinishRide(finish_ride) => {
                    ctx.address().do_send(finish_ride);
                }
                MessageType::DeclineRide(decline_ride) => {
                    ctx.address().do_send(decline_ride);
                }
                MessageType::PaymentRejected(payment_rejected) => {
                    ctx.address().do_send(payment_rejected);
                }
                _ => {
                    println!("Unknown Message");
                }
            }
        }
    }
}

/// Handles RideRequest messages coming from main and sends them to the TcpSender actor
impl Handler<RideRequest> for Passenger {
    type Result = ();

    fn handle(&mut self, msg: RideRequest, _ctx: &mut Self::Context) -> Self::Result {
        if self.state == Sates::Idle {
            println!("Passenger with id {} requested a ride", self.id);
            self.state = Sates::WaitingDriver;
            match self.tcp_sender.try_send(msg) {
                Ok(_) => (),
                Err(_) => println!("Error al enviar mensaje al TcpSender"),
            }
        } else {
            // se pushea para ejecutarlo cuando se hace el dropoff del viaje actual
            self.rides.push(msg);
        }
    }
}

impl Handler<FinishRide> for Passenger {
    type Result = ();

    fn handle(&mut self, msg: FinishRide, _ctx: &mut Self::Context) -> Self::Result {
        println!("Passenger with id {} finished ride with driver {}", msg.passenger_id, msg.driver_id);
        self.state = Sates::Idle;
        // TODO: hay que ver como manejarse aca, podria el pasajero leer su vector de rides y procesarlos? o solo 1 ride por pasajero
    }
}


impl Handler<DeclineRide> for Passenger {
    type Result = ();

    fn handle(&mut self, msg: DeclineRide, _ctx: &mut Self::Context) -> Self::Result {
        println!("Passenger with id {} got declined message from Leader {}", msg.passenger_id, msg.driver_id);
        self.state = Sates::Idle;
        // TODO: hay que ver como manejarse aca, podria el pasajero leer su vector de rides y procesarlos? o solo 1 ride por pasajero
    }
}

impl Handler<PaymentRejected> for Passenger {
    type Result = ();

    fn handle(&mut self, msg: PaymentRejected, _ctx: &mut Self::Context) -> Self::Result {}
}

impl Passenger {
    /// Creates the actor and connects to the leader
    /// # Arguments
    /// * `port` - The port of the passenger
    /// * `rides` - The list of rides (coordinates) that the passenger has to go to
    /// * `tx` - The channel to send a completion signal to the main function
    pub async fn start(port: u16, rides: Vec<RideRequest>) -> Result<(), io::Error> {
        let socket = TcpSocket::new_v4()?;
        socket.bind("127.0.0.1:9000".parse().unwrap())?; // Fija el puerto 12345
        let direction = format!("127.0.0.1:{}", crate::LEADER_PORT);
        let stream = socket.connect(direction.parse().unwrap()).await?;
        let rides_clone = rides.clone();
        let addr = Passenger::create_actor_instance(port, stream);

        // Send the rides to myself to be processed
        for ride in rides_clone {
            addr.send(ride).await.unwrap();
        }

        /// listen for connections
        let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).await?;

        /// Aca entrara solo cuando se caiga el lider, y un nuevo lider queira restablecer la conexion
        while let Ok((stream,  _)) = listener.accept().await {
            // TODO: cuando implemente mensajes, deberia escribirle al pasajero que se
            // cayo el antiguo lider y que se conecto uno nuevo
            // Deberia pasar el nuevo stream como un mensaje de actor, creo que en discord preguntaron
        }

        Ok(())
    }

    fn create_actor_instance(port: u16, stream: TcpStream) -> Addr<Passenger> {
        Passenger::create(|ctx| {
            let (read, write_half) = split(stream);
            Passenger::add_stream(LinesStream::new(BufReader::new(read).lines()), ctx);
            let write = Some(write_half);
            let addr_tcp = TcpSender::new(write).start();
            Passenger::new(port, addr_tcp)
        })
    }

    pub fn new(port: u16, tcp_sender: Addr<TcpSender>) -> Self {
        Passenger {
            id: port,
            leader_port: LEADER_PORT,
            tcp_sender,
            state: Sates::Idle,
            rides: Vec::new(),
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

/// Handles RideRequest messages coming from the Passenger actor and sends them to the leader
#[async_handler]
impl Handler<RideRequest> for TcpSender {
    type Result = ();

    async fn handle(&mut self, msg: RideRequest, ctx: &mut Self::Context) -> Self::Result {
        let mut write = self.write.take()
            .expect("No debería poder llegar otro mensaje antes de que vuelva por usar AtomicResponse");

        let msg_type = MessageType::RideRequest(msg);
        let serialized = serde_json::to_string(&msg_type).expect("should serialize");
        let ret_write = async move {
            write
                .write_all(format!("{}\n", serialized).as_bytes()).await
                .expect("should have sent");
            write
        }.await;

        self.write = Some(ret_write);
    }

}
