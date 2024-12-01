use actix::{Actor, Context, Handler, Addr};
use std::collections::HashMap;
use std::io::{self};
use tokio::io::{WriteHalf, ReadHalf, AsyncWriteExt, BufReader, AsyncBufReadExt};
use tokio::net::TcpStream;
use actix::StreamHandler;
use std::sync::Arc;
use actix_async_handler::async_handler;
use tokio::sync::RwLock;
use tokio_stream::wrappers::LinesStream;
use rand::Rng;
use colored::Colorize;

use crate::messages::*;

const PROBABILITY_PAYMENT_REJECTED: f64 = 0.01;


/// ----------------------------------    Socket Reader   ----------------------------------  ///

/// Contains the socket address and the address from the PaymentApp actor
/// Reads the new message from the socket address and sends it to the
/// paymentapp actor to process it
pub struct SocketReader {
    payment_app: Addr<PaymentApp>,
}


impl Actor for SocketReader {
    type Context = Context<Self>;
}

impl SocketReader {
    /// Creates a new SocketReader who will listen to the socket address passed by
    /// parameter, turn the received information to formated messages and send them
    /// to the payment app
    pub fn new(payment_app: Addr<PaymentApp>) -> Self {
        Self {payment_app}
    }

    /// Starts and initializes the SocketReader actor
    pub async fn start(read_half: ReadHalf<TcpStream>, payment_app: Addr<PaymentApp>) -> Result<(), io::Error> {
        SocketReader::create(|ctx| {
            SocketReader::add_stream(LinesStream::new(BufReader::new(read_half).lines()), ctx);
            SocketReader::new(payment_app)
        });
        Ok(())
    }

}

/// Handles the information received from the socket, descerialices it and
/// sends it to the payment app in a format that it will be able to process
impl StreamHandler<Result<String, io::Error>> for SocketReader {
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


/// ----------------------------------    Socket Writer   ----------------------------------  ///

/// Contains the wirte half of the socket
pub struct SocketWriter {
    write_half: Arc<RwLock<WriteHalf<TcpStream>>>,
}

impl Actor for SocketWriter {
    type Context = Context<Self>;
}

impl SocketWriter {
    /// Creates a new SocketWriter using the write half of the socket
    /// passed by parameter
    pub fn new(write_half: WriteHalf<TcpStream>) -> Self {
        Self { write_half: Arc::new(RwLock::new(write_half)) }
    }
}

//#[async_handler]
impl Handler<PaymentAccepted> for SocketWriter {
    type Result = ();

    /// Serializes and sends the message through the socket
    fn handle(&mut self, msg: PaymentAccepted, _: &mut Self::Context) -> Self::Result {
        //let mut write = self.write_half.take();
        let json_message = serde_json::to_string(&MessageType::PaymentAccepted(msg))
            .expect("Error serializing PaymentAccepted message");

        let write_half = self.write_half.clone();

        actix::spawn(async move {
            let mut write_half = write_half.write().await;

            // Write serialized msg in the socket
            if let Err(e) = write_half.write_all(format!("{}\n", json_message).as_bytes()).await {
                eprintln!("Error writing in socket: {:?}", e);
            }
            println!("Payment accepted sent {}", json_message);
        });

    }
}

impl Handler<PaymentRejected> for SocketWriter {
    type Result = ();

    /// Serializes and sends the message through the socket
    fn handle(&mut self, msg: PaymentRejected, _: &mut Self::Context) {
        let json_message = serde_json::to_string(&MessageType::PaymentRejected(msg))
            .expect("Error serializing PaymentRejected message");

        let write_half = self.write_half.clone();

        actix::spawn(async move {
            let mut write_half = write_half.write().await;

            // Writes serialized msg in the socket
            if let Err(e) = write_half.write_all(format!("{}\n", json_message).as_bytes()).await {
                eprintln!("Error writing in socket: {:?}", e);
            }
            println!("Payment rejected sent");
        });

    }
}



///  ----------------------------------    Payment App   ----------------------------------  ///

pub struct PaymentApp {
    rides_and_payments: HashMap<u16,u16>, //id, amount
    writer: Addr<SocketWriter>,
}

impl Actor for PaymentApp {
    type Context = Context<Self>;
}

impl Handler<SendPayment> for PaymentApp {
    type Result = ();

    /// Procesa el mensaje 'SendPayment' y envía al wirter si fue aceptado o no
    fn handle(&mut self, msg: SendPayment, _: &mut Self::Context) {
        if self.payment_is_accepted() {
            let payment_accepted = PaymentAccepted{id: msg.id, amount: msg.amount};
            log(&format!("PAYMENT ACCEPTED FOR RIDE {}", msg.id), "NEW_CONNECTION");
            self.rides_and_payments.insert(msg.id, msg.amount); //Lo agrego a los viajes aceptados
            self.writer.do_send(payment_accepted);
        }
        else {
            let payment_rejected = PaymentRejected{id:msg.id};
            log(&format!("PAYMENT REJECTED FOR RIDE {}", msg.id), "DISCONNECTION");
            self.writer.do_send(payment_rejected);
        }
    }
}

impl PaymentApp {
    // Nueva función `new` para PaymentApp
    pub fn new(writer: Addr<SocketWriter>) -> Self {
        PaymentApp {
            rides_and_payments: HashMap::new(),
            writer,
        }
    }

    pub fn payment_is_accepted(&mut self) -> bool {
        let mut rng = rand::thread_rng();
        let random_number: f64 = rng.gen_range(0.0..1.0);
        random_number > PROBABILITY_PAYMENT_REJECTED
    }
}



pub fn log(message: &str, type_msg: &str) {
    match type_msg {
        "DRIVER" => println!("[{}] - {}", type_msg, message.blue().bold()),
        "INFO" => println!("[{}] - {}", type_msg, message.cyan().bold()),
        "DISCONNECTION" => println!("[{}] - {}", type_msg, message.red().bold()), // disc = disconnection
        "NEW_CONNECTION" => println!("[{}] - {}", type_msg, message.green().bold()), // nc = no connection
        _ => println!("[{}] - {}", type_msg, message.green().bold()),
    }
}
