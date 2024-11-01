// creame un server que escuche en el puerto 6000 y acepte conexiones y printee lo que le envian

use std::io::{Read};
use std::net::{TcpListener};

fn main() {
    let listener = TcpListener::bind("127.0.0.1:6000").unwrap();
    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                let mut buffer = [0; 1024];
                stream.read(&mut buffer).unwrap();
                println!("Received: {}", String::from_utf8_lossy(&buffer));
            }
            Err(e) => {
                println!("Error: {}", e);
            }
        }
    }
}