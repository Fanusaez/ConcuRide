use actix::{Actor, Context, Handler, Message, Addr};
//use tokio::net::tcp::{WriteHalf, ReadHalf};
use std::collections::HashMap;
use std::io::{self};
use tokio::io::{WriteHalf, ReadHalf, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use serde::{Serialize, Deserialize};
use actix::StreamHandler;
use std::sync::Arc;
use tokio::sync::RwLock;



// Mensajes
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "message_type")]
enum MessageType {
    SendPayment(SendPayment),
    PaymentAccepted(PaymentAccepted),
    //PaymentRejected(PaymentRejected),
}

#[derive(Serialize, Deserialize, Message, Debug, Clone, Copy)]
#[rtype(result = "()")]
pub struct SendPayment {
    pub id: u16,
    pub amount: i32,
}

#[derive(Serialize, Deserialize, Message, Debug, Clone, Copy)]
#[rtype(result = "()")]
pub struct PaymentAccepted {
    pub id: u16,
}

#[derive(Serialize, Deserialize, Message, Debug, Clone, Copy)]
#[rtype(result = "()")]
pub struct PaymentRejected {
    pub id: u16,
}

// Mensajes que enviará el lector al PaymentApp
//#[derive(Message, Debug)]
//#[rtype(result = "()")]
//struct IncomingMessage(MessageType);







// Actor lector
pub struct SocketReader {
    read_half: ReadHalf<TcpStream>,
    payment_app: Addr<PaymentApp>,
}

impl Actor for SocketReader {
    type Context = Context<Self>;
}

impl SocketReader {
    pub fn new(read_half: ReadHalf<TcpStream>, payment_app: Addr<PaymentApp>) -> Self {
        // Aquí leerías los mensajes del socket en un bucle y los reenviarías a PaymentApp
        // usando `payment_app.do_send(IncomingMessage(...))`.
        Self {read_half, payment_app}
    }

}

impl StreamHandler<Result<String, io::Error>> for SocketReader {
    fn handle(&mut self, read: Result<String, io::Error>, _ctx: &mut Self::Context) {
        if let Ok(line) = read {
            println!("{}", line);
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







// Actor escritor
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

// Mensajes que PaymentApp puede enviar a SocketWriter
//#[derive(Message, Debug)]
//#[rtype(result = "()")]
//struct OutgoingMessage(MessageType);

impl Handler<PaymentAccepted> for SocketWriter {
    type Result = ();

    /// Serializa y envía el mensaje por el socket
    fn handle(&mut self, msg: PaymentAccepted, _: &mut Self::Context) {
        let json_message = serde_json::to_string(&MessageType::PaymentAccepted(msg))
            .expect("Error serializando el mensaje PaymentAccepted");

        // Lanza una tarea asincrónica para escribir en el socket
        let write_half = self.write_half.clone(); // Necesitamos clonar el Arc para moverlo al async block

        actix::spawn(async move {
            let mut write_half = write_half.write().await; // Usamos RwLock de Tokio

            // Escribir el mensaje serializado en el socket
            if let Err(e) = write_half.write_all(json_message.as_bytes()).await {
                eprintln!("Error escribiendo en el socket: {:?}", e);
            }
            println!("Pago aceptado enviado");
        });

    }
}







pub struct PaymentApp {
    rides_and_payments: HashMap<u16,u16>,
    writer: Addr<SocketWriter>,
}

impl Actor for PaymentApp {
    type Context = Context<Self>;
}

impl Handler<SendPayment> for PaymentApp {
    type Result = ();

    /// Procesa el mensaje 'SendPayment' y envía al wirter si fue aceptado o no
    fn handle(&mut self, msg: SendPayment, _: &mut Self::Context) {
        let payment_accepted = PaymentAccepted{id: msg.id};
        println!("Pago aceptado");
        self.rides_and_payments.insert(msg.id,1000); //Lo agrego a los viajes aceptados
        println!("Pago agregado");
        self.writer.do_send(payment_accepted);
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
