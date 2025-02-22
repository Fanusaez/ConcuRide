use crate::driver::{DriverStatus, FullStream};
use std::collections::HashMap;
use std::io;
use tokio::io::{split, ReadHalf, WriteHalf};
use tokio::net::TcpStream;

const PAYMENT_APP_PORT: u16 = 7500;

pub async fn init_driver(
    drivers_connections: &mut HashMap<u16, FullStream>,
    drivers_ports: Vec<u16>,
    drivers_last_position: &mut HashMap<u16, (i32, i32)>,
    is_leader: bool,
    payment_write_half: &mut Option<WriteHalf<TcpStream>>,
    payment_read_half: &mut Option<ReadHalf<TcpStream>>,
    drivers_status: &mut HashMap<u16, DriverStatus>,
) -> Result<(), io::Error> {
    if is_leader {
        for driver_port in drivers_ports.iter() {
            drivers_status.insert(
                *driver_port,
                DriverStatus {
                    last_response: Some(std::time::Instant::now()),
                    is_alive: true,
                },
            );
        }
        for driver_port in drivers_ports.iter() {
            let stream = TcpStream::connect(format!("127.0.0.1:{}", driver_port)).await?;
            let (read_half, write_half) = split(stream);
            drivers_connections.insert(*driver_port, (Some(read_half), Some(write_half)));
            drivers_last_position.insert(*driver_port, (0, 0));
        }
        // Conexión con el servicio de pagos
        let payment_stream = TcpStream::connect(format!("127.0.0.1:{}", PAYMENT_APP_PORT)).await?;
        let (read_half, write_half) = split(payment_stream);
        *payment_write_half = Some(write_half);
        *payment_read_half = Some(read_half);

        Ok(())
    } else {
        Ok(())
    }
}
