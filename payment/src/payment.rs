//! Payment app actors (SocketReader, PaymentApp, SocketWriter)

use actix::{Actor, Context, Handler, Addr, AsyncContext, WrapFuture};
use std::collections::HashMap;
use std::io::{self};
use tokio::io::{WriteHalf, ReadHalf, AsyncWriteExt, BufReader, AsyncBufReadExt};
use tokio::net::TcpStream;
use actix::StreamHandler;
use tokio_stream::wrappers::LinesStream;
use rand::Rng;
use colored::Colorize;
use actix::ActorFutureExt;

use crate::messages::*;

/// Probability that a payment will be rejected
const PROBABILITY_PAYMENT_REJECTED: f64 = 0.01;

// ---------------------------------------------------------------------------------------- //
// ----------------------------------    Socket Reader   ---------------------------------- //
// ---------------------------------------------------------------------------------------- //


/// Contains the socket address and the address from the PaymentApp actor
/// Reads the new message from the socket address and sends it to the
/// paymentapp actor to process it
pub struct SocketReader {
    /// Adress of the actor PaymentApp
    payment_app: Addr<PaymentApp>,
}


impl Actor for SocketReader {
    type Context = Context<Self>;
}

impl SocketReader {
    /// Creates a new SocketReader who will listen to the socket address passed by
    /// parameter, turn the received information to formated messages and send them
    /// to the payment app
    /// # Arguments
    /// `payment_app` - Address of the actor PaymentApp
    /// # Returns
    /// `SocketReader`
    pub fn new(payment_app: Addr<PaymentApp>) -> Self {
        Self {payment_app}
    }

    /// Starts and initializes the SocketReader actor
    /// # Arguments
    pub async fn start(read_half: ReadHalf<TcpStream>, payment_app: Addr<PaymentApp>) -> Result<(), io::Error> {
        SocketReader::create(|ctx| {
            SocketReader::add_stream(LinesStream::new(BufReader::new(read_half).lines()), ctx);
            SocketReader::new(payment_app)
        });
        Ok(())
    }

}

impl StreamHandler<Result<String, io::Error>> for SocketReader {
    /// Handles the information received from the socket, descerialices it and
    /// sends it to the payment app in a format that it will be able to process
    fn handle(&mut self, read: Result<String, io::Error>, _ctx: &mut Self::Context) {
        if let Ok(line) = read {

            let message: MessageType = serde_json::from_str(&line).expect("Failed to deserialize message");
            match message {
                MessageType::SendPayment(message) => {
                    self.payment_app.do_send(message);
                }
                _ => {
                    println!("Error")
                }
            }
        } else {
            println!("Failed to read line {:?}", read);
        }
    }
}

// ---------------------------------------------------------------------------------------- //
// ----------------------------------    Socket Writer   ---------------------------------- //
// ---------------------------------------------------------------------------------------- //

/// Contains the wirte half of the socket
pub struct SocketWriter {
    /// Tcp write half to send information to the leader driver
    write_half: Option<WriteHalf<TcpStream>>,
}

impl Actor for SocketWriter {
    type Context = Context<Self>;
}

impl SocketWriter {
    /// Creates a new SocketWriter using write half of the socket
    /// passed by parameter
    /// # Arguments
    /// `write_half` - Tcp write half option
    /// # Returns
    /// `SocketWriter` able to communicate to the leader driver
    pub fn new(write_half: Option<WriteHalf<TcpStream>>) -> Self {
        Self { write_half }
    }
}

impl Handler<PaymentAccepted> for SocketWriter {
    type Result = ();

    /// Serializes and sends the message through the socket
    fn handle(&mut self, msg: PaymentAccepted, ctx: &mut Context<Self>) -> Self::Result {
        let json_message = serde_json::to_string(&MessageType::PaymentAccepted(msg))
            .expect("Error serializing PaymentAccepted message");
        ctx.address().do_send(SendMessage {msg: json_message});
    }
}

impl Handler<PaymentRejected> for SocketWriter {
    type Result = ();

    /// Serializes and sends the message through the socket
    fn handle(&mut self, msg: PaymentRejected, ctx: &mut Context<Self>) -> Self::Result {
        let json_message = serde_json::to_string(&MessageType::PaymentRejected(msg))
            .expect("Error serializing PaymentRejected message");
        ctx.address().do_send(SendMessage {msg: json_message});

    }
}

impl Handler<SendMessage> for SocketWriter {
    type Result = ();
    fn handle(&mut self, msg: SendMessage, ctx: &mut Context<Self>) {
        let  write_half = self.write_half.take().expect("Writer already closed!");
        ctx.spawn(
            async move {
                let mut write = write_half;

                if let Err(e) = write
                    .write_all(format!("{}\n", msg.msg).as_bytes())
                    .await
                {
                    log::debug!("Error sending PaymentAccepted message: {}", e);
                }

                // Return the write_half after sending
                write
            }
                .into_actor(self) // Convert the future into an actor's future
                .map(|write_half, actor, _| {
                    // Reinsert the write_half into the actor
                    actor.write_half = Some(write_half);
                }),
        );
    }
}


// -------------------------------------------------------------------------------------- //
// ----------------------------------    Payment App   ---------------------------------- //
// -------------------------------------------------------------------------------------- //

/// Contains the rides whose payment have been approved and the address of the
/// SocketWriter actor
pub struct PaymentApp {
    /// Rides whose payment have already been approved <ride_id, price_amount>
    rides_and_payments: HashMap<u16,u16>, //id, amount
    /// Address of the SocketWriter actor to send information
    writer: Addr<SocketWriter>,
}

impl Actor for PaymentApp {
    type Context = Context<Self>;
}

impl Handler<SendPayment> for PaymentApp {
    type Result = ();

    /// Process `SendPayment` message and sends to the SocketWriter if the
    /// payment has been approved or not
    fn handle(&mut self, msg: SendPayment, _: &mut Self::Context) -> Self::Result {
        if self.payment_is_accepted() {
            let payment_accepted = PaymentAccepted{id: msg.id, amount: msg.amount};
            log(&format!("PAYMENT ACCEPTED FOR RIDE {}", msg.id), "NEW_CONNECTION");
            self.rides_and_payments.insert(msg.id, msg.amount); // Add to accepted rides
            match self.writer.try_send(payment_accepted) {
                Ok(_) => (),
                Err(_) => println!("Error sending message to SocketWriter")
            }
        }
        else {
            let payment_rejected = PaymentRejected{id:msg.id};
            log(&format!("PAYMENT REJECTED FOR RIDE {}", msg.id), "DISCONNECTION");
            match self.writer.try_send(payment_rejected) {
                Ok(_) => (),
                Err(_) => println!("Error sending message to SocketWriter")
            }
        }
    }
}

impl PaymentApp {
    /// Returns an instance of a PaymentApp
    pub fn new(writer: Addr<SocketWriter>) -> Self {
        PaymentApp {
            rides_and_payments: HashMap::new(),
            writer,
        }
    }

    /// Decides if the payment has been approved or not using a random generator
    pub fn payment_is_accepted(&mut self) -> bool {
        let mut rng = rand::thread_rng();
        let random_number: f64 = rng.gen_range(0.0..1.0);
        random_number > PROBABILITY_PAYMENT_REJECTED
    }
}


/// Log function to show formatted messages
pub fn log(message: &str, type_msg: &str) {
    match type_msg {
        "DRIVER" => println!("[{}] - {}", type_msg, message.blue().bold()),
        "INFO" => println!("[{}] - {}", type_msg, message.cyan().bold()),
        "DISCONNECTION" => println!("[{}] - {}", type_msg, message.red().bold()), // disc = disconnection
        "NEW_CONNECTION" => println!("[{}] - {}", type_msg, message.green().bold()), // nc = no connection
        _ => println!("[{}] - {}", type_msg, message.green().bold()),
    }
}
