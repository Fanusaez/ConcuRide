use std::cmp::PartialEq;
use std::io;
use actix::{Actor, Context, StreamHandler, Handler, Addr, AsyncContext, ActorContext};
use actix_async_handler::async_handler;
use tokio::io::{split, AsyncBufReadExt, AsyncWriteExt, BufReader, WriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio_stream::wrappers::LinesStream;
use log::debug;
use crate::{utils, LEADER_PORT};
use crate::models::*;


pub enum Sates {
    /// Before requesting a ride or after drop off
    Idle,
    /// After requesting a ride and paying, waiting for a driver to accept the ride
    WaitingDriver,
    /// Riding in a car
    Traveling,
}

/// Implement PartialEq for the Sates enum
impl PartialEq for Sates {
    fn eq(&self, other: &Self) -> bool {
        matches!((self, other), (Sates::Idle, Sates::Idle) |
            (Sates::WaitingDriver, Sates::WaitingDriver) |
            (Sates::Traveling, Sates::Traveling))
    }
}

/// Contains the passenger id, the addres of the Tcp Sender actor,
/// the possible states of the passenger and the rides that the
/// passenger will make
pub struct Passenger {
    /// The port of the passenger
    id: u16,
    /// The actor that sends messages to the leader
    tcp_sender: Addr<TcpSender>,
    /// State of the passenger
    state: Sates,
    /// Next Drive Request to be processed
    rides: Vec<RideRequest>,
}

impl Actor for Passenger {
    type Context = Context<Self>;

    /// Override the stopping method to keep the actor running
    fn stopping(&mut self, _: &mut Self::Context) -> actix::Running {
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
                Err(_) => debug!("Error sending message to TcpSender"),
            }
        } else {
            // If the passenger is idle, add the ride to the queue for later use
            self.rides.push(msg);
        }
    }
}

/// Handles FinishRide messages coming from the leader
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

/// Handles DeclineRide messages coming from the leader
impl Handler<DeclineRide> for Passenger {
    type Result = ();

    fn handle(&mut self, msg: DeclineRide, _ctx: &mut Self::Context) -> Self::Result {
        println!("Passenger with id {} got declined message from Leader {}", msg.passenger_id, msg.driver_id);
        self.state = Sates::Idle;
    }
}

/// Handles PaymentRejected messages coming from the leader
impl Handler<PaymentRejected> for Passenger {
    type Result = ();

    fn handle(&mut self, _msg: PaymentRejected, _ctx: &mut Self::Context) -> Self::Result {
        utils::log(&format!("PASSENGER WITH ID {} PAYMENT WAS REJECTED", self.id), "INFO");
        self.state = Sates::Idle;
    }
}

/// Handles NewConnection messages coming from main
/// Sends the message of NewConnection to the TcpSender actor
impl Handler<NewConnection> for Passenger {
    type Result = ();

    fn handle(&mut self, msg: NewConnection, _ctx: &mut Self::Context) -> Self::Result {
        match self.tcp_sender.try_send(msg) {
            Ok(_) => (),
            Err(_) => debug!("Error sending message to TcpSender"),
        }
    }
}

/// Handles NewLeaderStreams messages
/// Adds the new stream to the actor and creates a new TcpSender actor with the new write half of the leader
/// it's an auto message that is sent when a new leader is appointed
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
        // Stop the current TcpSender actor
        self.tcp_sender.do_send(StopActor);
        // Create a new TcpSender actor with the new write half of the leader
        let addr_tcp = TcpSender::new(write).start();
        self.tcp_sender = addr_tcp;
    }
}

/// Handles RideRequestReconnection messages coming from the leader
/// If passenger disconnected and reconnected wit a RideRequest ongoing, this message will arrive
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

        // listen for connections
        let listener = TcpListener::bind(format!("127.0.0.1:{}", port_id)).await?;

        // loop to accept all incoming connections, only used when new leader is appointed
        while let Ok((stream,  _)) = listener.accept().await {
            let (read, write) = split(stream);

            // add the new stream to the actor
            addr.send(NewLeaderStreams {
                read: Some(read),
                write_half: Some(write),
            }).await.unwrap();

        }

        Ok(())
    }

    /// Creates an actor Passenger instance
    /// # Arguments
    /// `port` - The port that the actor will be listening in case the leader driver
    /// disconnects
    /// `stream` - The Tcp Stream from which the passenger will listen to new messages
    /// from the driver or send them to him
    /// # Returns
    /// addres of the new passenger actor
    fn create_actor_instance(port: u16, stream: TcpStream) -> Addr<Passenger> {
        Passenger::create(|ctx| {
            let (read, write_half) = split(stream);
            Passenger::add_stream(LinesStream::new(BufReader::new(read).lines()), ctx);
            let write = Some(write_half);
            let addr_tcp = TcpSender::new(write).start();
            Passenger::new(port, addr_tcp)
        })
    }

    /// Creates a new Passenger
    /// # Arguments
    /// `port` - The port that the actor will be listening in case the leader driver
    /// disconnects
    /// `tcp_sender` - Address of the actor that will send messages to the leader
    /// driver through the socket
    pub fn new(port: u16, tcp_sender: Addr<TcpSender>) -> Self {
        Passenger {
            id: port,
            tcp_sender,
            state: Sates::Idle,
            rides: Vec::new(),
        }
    }
}


/// Contains an Option of the write half of a tcp stream
pub struct TcpSender {
    /// The write half of the TcpStream
    write: Option<WriteHalf<TcpStream>>,
}

/// The actor that sends messages to the leader
impl Actor for TcpSender {
    type Context = Context<Self>;
}

impl TcpSender {
    /// Returns a TcpSender
    /// # Arguments
    /// `write` - Option of a WriteHalf of a TcpStream that the TcpSender will
    /// use to send messages
    pub fn new(write: Option<WriteHalf<TcpStream>>) -> Self {
        TcpSender { write }
    }
}

/// Handles RideRequest messages coming from the Passenger actor and sends them to the leader
/// todo: aca hay que hacer un refactor, si lo unico que hace tcp sender es enviar los mensajes al lider
#[async_handler]
impl Handler<RideRequest> for TcpSender {
    type Result = ();

    async fn handle(&mut self, msg: RideRequest, _ctx: &mut Self::Context) -> Self::Result {
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

/// Handles `NewConnection` message coming from Passenger and sends it to the
/// leader driver through the socket
#[async_handler]
impl Handler<NewConnection> for TcpSender {
    type Result = ();

    async fn handle(&mut self, msg: NewConnection, _ctx: &mut Self::Context) -> Self::Result {
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

/// Handles StopActor messages coming from the Passenger actor
impl Handler<StopActor> for TcpSender {
    type Result = ();
    fn handle(&mut self, _msg: StopActor, ctx: &mut Self::Context) -> Self::Result {
        ctx.stop();
    }
}