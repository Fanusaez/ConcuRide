use std::collections::HashMap;
use std::io;
use std::sync::{Arc, RwLock};
use tokio::io::{split, ReadHalf, WriteHalf};
use tokio::net::TcpStream;

pub async fn init_driver(drivers_connections: &mut HashMap<u16, (Option<ReadHalf<TcpStream>>, Option<WriteHalf<TcpStream>>)>,
                    drivers_ports: Vec<u16>,
                    drivers_last_position: &mut HashMap<u16, (i32, i32)>,
                    is_leader: bool) -> Result<(), io::Error> {
    if is_leader {
        for driver_port in drivers_ports.iter() {
            let stream = TcpStream::connect(format!("127.0.0.1:{}", driver_port)).await?;
            let (read_half, write_half) = split(stream);
            drivers_connections.insert(*driver_port, (Some(read_half), Some(write_half)));
            drivers_last_position.insert(*driver_port, (0, 0));
        }
    }
    else {
        // TODO funcionalidad del driver que no es lider
    }
    Ok(())
}
