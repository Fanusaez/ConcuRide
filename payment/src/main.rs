use std::env;
use crate::payment::PaymentApp;
use actix::prelude::*;
use tokio::net::TcpStream;
use tokio::io::{split, ReadHalf, WriteHalf};
//use std::sync::Arc;
//use std::sync::RwLock;
//use std::io; 

mod payment;

use crate::payment::{SocketWriter, SocketReader};

const DEFAULT_PORT: u16 = 7500;
const LEADER_PORT: u16 = 6000;

#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();

    let port = if args.len() == 1 {
        DEFAULT_PORT
    } else {
        args[1].parse::<u16>().expect("Ingrese un numero de puerto")
    };

    println!("Usando el puerto: {}", port);

    let stream = TcpStream::connect(format!("127.0.0.1:{}", LEADER_PORT)).await?;
    let (read_half, write_half) = split(stream);

    // Creo el socket writer (encargado de recibir los mensajes de payment app y enviarlos
    // por socket al conductor lider)
    let writer_addr = SocketWriter::new(write_half).start();

    // Creo la app de pagos y le paso la direccion del socket writer para enviarle los mensajes
    let payment_app = PaymentApp::new(writer_addr).start();

    // Creo el actor SocketReader, pasando el read_half y la dirección de PaymentApp
    let _reader_addr = SocketReader::new(read_half, payment_app.clone()).start();

    //System::current().stop();
    Ok(())


    //PaymentApp::new(port, LEADER_PORT);
    //Ok(())
}