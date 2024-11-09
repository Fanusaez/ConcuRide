use std::collections::HashMap;
use std::io;
use tokio::io::{split, WriteHalf};
use tokio::net::TcpStream;

pub async fn init_driver(drivers_connections: &mut HashMap<u16, Option<WriteHalf<TcpStream>>>,
                    drivers_ports: Vec<u16>,
                    is_leader: bool) -> Result<(), io::Error> {
    if is_leader {
        for driver_port in drivers_ports.iter() {
            let stream = TcpStream::connect(format!("127.0.0.1:{}", driver_port)).await?;
            let (_, write_half) = split(stream);
            drivers_connections.insert(*driver_port, Some(write_half));
        }
    }
    else {
        // TODO funcionalidad del driver que no es lider
    }
    Ok(())
}
