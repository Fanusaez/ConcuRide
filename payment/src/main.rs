use std::env;
use crate::payment::PaymentApp;
use actix::prelude::*;
use tokio::net::TcpStream;
use tokio::io::{split, ReadHalf, WriteHalf};
use actix::{Actor, Addr};
//use std::sync::Arc;
//use std::sync::RwLock;
//use std::io; 
use tokio::net::TcpListener;
use std::net::SocketAddr;


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

    let listener = TcpListener::bind(format!("127.0.0.1:{}", DEFAULT_PORT)).await.unwrap();

    while let Ok((stream, addr)) = listener.accept().await {
        let (read_half, write_half) = split(stream);
        println!("[{:?}] Cliente conectado", addr);
        // Creo el socket writer (encargado de recibir los mensajes de payment app y enviarlos
        // por socket al conductor lider)
        let writer_addr = SocketWriter::new(write_half).start();

        // Creo la app de pagos y le paso la direccion del socket writer para enviarle los mensajes
        let payment_app_addr = PaymentApp::new(writer_addr).start();
    
        // Creo el actor SocketReader, pasando el read_half y la dirección de PaymentApp
        SocketReader::start(read_half, addr, payment_app_addr).await?;
    }

    println!("b1");

    Ok(())


    //PaymentApp::new(port, LEADER_PORT);
    //Ok(())
}