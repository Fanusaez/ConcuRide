use std::cmp::PartialEq;
use std::hash::Hash;
use std::io;
use std::thread::sleep;
use actix::{Actor, Context, StreamHandler, ActorFutureExt, Handler, Addr, AsyncContext};
use actix_async_handler::async_handler;
use tokio::io::{split, AsyncBufReadExt, AsyncWriteExt, BufReader, WriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio_stream::wrappers::LinesStream;
use tokio::net::TcpSocket;
use tokio::net::unix::pid_t;
use crate::{utils, LEADER_PORT};
use crate::models::*;


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

    fn stopping(&mut self, _: &mut Self::Context) -> actix::Running {
        // Evita que el actor muera mientras tenga streams activos
        // TODO: puse esta condicion rando, se podria poner otra que sea mas logica
        actix::Running::Continue
    }

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

                MessageType::RideRequestReconnection(ride_request_reconnection) => {
                    ctx.address().do_send(ride_request_reconnection);
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
            utils::log(&format!("PASSENGER WITH ID {} REQUESTED A RIDE", self.id), "INFO");
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

    fn handle(&mut self, msg: FinishRide, ctx: &mut Self::Context) -> Self::Result {
        utils::log(&format!("PASSENGER WITH ID {} FINISHED RIDE WITH DRIVER {}", msg.passenger_id, msg.driver_id), "INFO");
        self.state = Sates::Idle;
        let addr = ctx.address();
        if let Some(ride) = self.rides.pop() {
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                addr.do_send(ride)
            });
        }
    }
}


impl Handler<DeclineRide> for Passenger {
    type Result = ();

    fn handle(&mut self, msg: DeclineRide, _ctx: &mut Self::Context) -> Self::Result {
        println!("Passenger with id {} got declined message from Leader {}", msg.passenger_id, msg.driver_id);
        self.state = Sates::Idle;
    }
}

impl Handler<PaymentRejected> for Passenger {
    type Result = ();

    fn handle(&mut self, msg: PaymentRejected, _ctx: &mut Self::Context) -> Self::Result {
        utils::log(&format!("PASSENGER WITH ID {} PAYMENT WAS REJECTED", self.id), "INFO");
        self.state = Sates::Idle;
    }
}

impl Handler<NewConnection> for Passenger {
    type Result = ();

    fn handle(&mut self, msg: NewConnection, _ctx: &mut Self::Context) -> Self::Result {
        match self.tcp_sender.try_send(msg) {
            Ok(_) => (),
            Err(_) => println!("Error al enviar mensaje al TcpSender"),
        }
    }
}

impl Handler<NewLeaderStreams> for Passenger {
    type Result = ();
    fn handle(&mut self, msg: NewLeaderStreams, _ctx: &mut Self::Context) -> Self::Result {
        utils::log("NEW LEADER APPOINTED", "CONNECTION");
        if let Some(read_half) = msg.read {
            Passenger::add_stream(LinesStream::new(BufReader::new(read_half).lines()), _ctx);
        } else {
            eprintln!("No se proporcionó un stream válido");
        }
        let write = msg.write_half;
        let addr_tcp = TcpSender::new(write).start();
        self.tcp_sender = addr_tcp;

    }
}

impl Handler<RideRequestReconnection> for Passenger {
    type Result = ();

    fn handle(&mut self, msg: RideRequestReconnection, _ctx: &mut Self::Context) -> Self::Result {
        utils::log("RIDE REQUEST RECONNECTED", "INFO");
        match msg.state.as_str() {
            "Idle" => {
                self.state = Sates::Idle;
            }
            "WaitingDriver" => {
                self.state = Sates::WaitingDriver;
            }
            "Traveling" => {
                self.state = Sates::Traveling;
            }
            _ => {
                println!("Unknown state");
            }
        }
    }
}

impl Passenger {
    /// Creates the actor and connects to the leader
    /// # Arguments
    /// * `port` - The port of the passenger
    /// * `rides` - The list of rides (coordinates) that the passenger has to go to
    /// * `tx` - The channel to send a completion signal to the main function
    pub async fn start(port_id: u16, rides: Vec<RideRequest>) -> Result<(), io::Error> {
        let stream = TcpStream::connect(format!("127.0.0.1:{}", LEADER_PORT)).await?;
        let used_port = stream.local_addr()?.port();
        let rides_clone = rides.clone();
        let addr = Passenger::create_actor_instance(port_id, stream);

        let msg = NewConnection {
            passenger_id: port_id,
            used_port,
        };

        addr.send(msg).await.unwrap();


        // Send the rides to myself to be processed
        for ride in rides_clone {
            addr.send(ride).await.unwrap();
        }

        /// listen for connections
        let listener = TcpListener::bind(format!("127.0.0.1:{}", port_id)).await?;

        /// Aca entrara solo cuando se caiga el lider, y un nuevo lider queira restablecer la conexion
        while let Ok((stream,  _)) = listener.accept().await {

            // TODO: cuando implemente mensajes, deberia escribirle al pasajero que se
            // cayo el antiguo lider y que se conecto uno nuevo
            // Deberia pasar el nuevo stream como un mensaje de actor, creo que en discord preguntaron
            let (read, mut write) = split(stream);

            // agrego el stream del nuevo lider
            addr.send(NewLeaderStreams {
                read: Some(read),
                write_half: Some(write),
            }).await.unwrap();

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

#[async_handler]
impl Handler<NewConnection> for TcpSender {
    type Result = ();

    async fn handle(&mut self, msg: NewConnection, ctx: &mut Self::Context) -> Self::Result {
        let mut write = self.write.take()
            .expect("No debería poder llegar otro mensaje antes de que vuelva por usar AtomicResponse");

        let msg_type = MessageType::NewConnection(msg);
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