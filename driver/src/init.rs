use std::collections::HashMap;
use std::io;
use std::sync::{Arc, RwLock};
use tokio::io::{split, ReadHalf, WriteHalf};
use tokio::net::TcpStream;

const PAYMENT_APP_PORT: u16 = 7500;

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
        // Conexión con el servicio de pagos (TODO: DEBERIA CONECTARSE SOLO SI ES LIDER)
        let payment_stream = TcpStream::connect(format!("127.0.0.1:{}", PAYMENT_APP_PORT)).await?;
        let (_, payment_write_half) = split(payment_stream);
        let payment_write_half = Arc::new(RwLock::new(Some(payment_write_half)));
        return Ok(())
    }
    else {
        // TODO funcionalidad del driver que no es lider
        return Ok(())
    }
}
