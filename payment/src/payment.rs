use actix::{Actor, Context, Handler, Message, Addr};
//use tokio::net::tcp::{WriteHalf, ReadHalf};
use std::collections::HashMap;
use std::io::{self};
use tokio::io::{WriteHalf, ReadHalf, AsyncWriteExt, BufReader, AsyncBufReadExt};
use tokio::net::TcpStream;
use serde::{Serialize, Deserialize};
use actix::StreamHandler;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::net::SocketAddr;
use tokio_stream::wrappers::LinesStream;
use rand::Rng;

const PROBABILITY_PAYMENT_REJECTED: f64 = 0.01;


// Mensajes
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "message_type")]
enum MessageType {
    SendPayment(SendPayment),
    PaymentAccepted(PaymentAccepted),
    PaymentRejected(PaymentRejected),
}

#[derive(Serialize, Deserialize, Message, Debug, Clone, Copy)]
#[rtype(result = "()")]
pub struct SendPayment {
    pub id: u16,
    pub amount: u16,
}

#[derive(Serialize, Deserialize, Message, Debug, Clone, Copy)]
#[rtype(result = "()")]
pub struct PaymentAccepted {
    pub id: u16,
    pub amount: u16,
}

#[derive(Serialize, Deserialize, Message, Debug, Clone, Copy)]
#[rtype(result = "()")]
pub struct PaymentRejected {
    pub id: u16,
}

/// ----------------------------------    Socket Reader   ----------------------------------  ///

// Actor lector
pub struct SocketReader {
    addr: SocketAddr,
    payment_app: Addr<PaymentApp>,
}

impl Actor for SocketReader {
    type Context = Context<Self>;
}

impl SocketReader {
    pub fn new(addr: SocketAddr, payment_app: Addr<PaymentApp>) -> Self {
        Self {addr, payment_app}
    }

    pub async fn start(read_half: ReadHalf<TcpStream>, addr: SocketAddr, payment_app: Addr<PaymentApp>) -> Result<(), io::Error> {
        SocketReader::create(|ctx| {
            SocketReader::add_stream(LinesStream::new(BufReader::new(read_half).lines()), ctx);
            SocketReader::new(addr, payment_app)
        });
        Ok(())
    }

}

impl StreamHandler<Result<String, io::Error>> for SocketReader {
    fn handle(&mut self, read: Result<String, io::Error>, _ctx: &mut Self::Context) {
        if let Ok(line) = read {

            println!("Mensaje de {}: {}", self.addr, line);

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

pub struct SocketWriter {
    write_half: Arc<RwLock<WriteHalf<TcpStream>>>,
}

impl Actor for SocketWriter {
    type Context = Context<Self>;
}

impl SocketWriter {
    pub fn new(write_half: WriteHalf<TcpStream>) -> Self {
        Self { write_half: Arc::new(RwLock::new(write_half)) }
    }
}

impl Handler<PaymentAccepted> for SocketWriter {
    type Result = ();

    /// Serializa y envía el mensaje por el socket
    fn handle(&mut self, msg: PaymentAccepted, _: &mut Self::Context) {
        let json_message = serde_json::to_string(&MessageType::PaymentAccepted(msg))
        .expect("Error serializando el mensaje PaymentAccepted");

        // Lanza una tarea asincrónica para escribir en el socket
        let write_half = self.write_half.clone();

        actix::spawn(async move {
            let mut write_half = write_half.write().await;
            
            // Escribir el mensaje serializado en el socket
            if let Err(e) = write_half.write_all(format!("{}\n", json_message).as_bytes()).await {
                eprintln!("Error escribiendo en el socket: {:?}", e);
            }
            println!("Payment accepted sent {}", json_message);
        });

    }
}

impl Handler<PaymentRejected> for SocketWriter {
    type Result = ();

    /// Serializa y envía el mensaje por el socket
    fn handle(&mut self, msg: PaymentRejected, _: &mut Self::Context) {
        let json_message = serde_json::to_string(&MessageType::PaymentRejected(msg))
            .expect("Error serializando el mensaje PaymentAccepted");

        // Lanza una tarea asincrónica para escribir en el socket
        let write_half = self.write_half.clone();

        actix::spawn(async move {
            let mut write_half = write_half.write().await;

            // Escribir el mensaje serializado en el socket
            if let Err(e) = write_half.write_all(format!("{}\n", json_message).as_bytes()).await {
                eprintln!("Error escribiendo en el socket: {:?}", e);
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
            println!("Pago accepted");
            self.rides_and_payments.insert(msg.id, msg.amount); //Lo agrego a los viajes aceptados
            self.writer.do_send(payment_accepted);
        }
        else {
            let payment_rejected = PaymentRejected{id:msg.id};
            println!("Payment rejected");
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









/*
// Mensaje para configurar el escritor en PaymentApp
#[derive(Message)]
#[rtype(result = "()")]
pub struct SetWriter(Addr<SocketWriter>);

impl Handler<SetWriter> for PaymentApp {
    type Result = ();
    
    fn handle(&mut self, msg: SetWriter, _: &mut Context<Self>) {
        self.writer = Some(msg.0);
    }
}
*/

/*
pub struct PaymentApp {
    rides_and_payments: HashMap<u16, u16>,
    write_half: Option<WriteHalf<TcpStream>>,
    read_half: Option<ReadHalf<TcpStream>>,
}

impl Actor for PaymentApp {
    type Context = Context<Self>;
}

impl PaymentApp {
    pub async fn new(port: u16, leader_port: u16) -> Result<Self, Box<dyn std::error::Error>> {
        let stream = TcpStream::connect(format!("127.0.0.1:{}", leader_port)).await?;
        let (r_half, w_half) = split(stream);
        Ok(PaymentApp { rides_and_payments: HashMap::new(), read_half: Some(r_half), write_half: Some(w_half) })
    }
    
    pub fn send_message(&mut self, message: MessageType) -> Result<(), io::Error> {
        let serialized = serde_json::to_string(&message)?;
        self.write_half.write_all(format!("{}\n", serialized).as_bytes())?;
        Ok(())
    }
}

impl Handler<SendPayment> for PaymentApp {
    type Result = ();
    
    fn handle(&mut self, msg: SendPayment, _ctx: &mut Self::Context) {
        
}
}

impl StreamHandler<Result<String, io::Error>> for PaymentApp {
    /// Handles the messages coming from the associated stream.
    /// Matches the message type and sends it to the corresponding handler.
    fn handle(&mut self, read: Result<String, io::Error>, ctx: &mut Self::Context) {
        if let Ok(line) = read {
            let message: MessageType = serde_json::from_str(&line).expect("Failed to deserialize message");
            match message {
                MessageType::SendPayment(payment)=> {
                    ctx.address().do_send(payment);
                }
                _ => {
                    println!("Failed to read message");
                } 
            }
        } else {
            println!("[{:?}] Failed to read line {:?}", self.id, read);
        }
    }
}
*/
