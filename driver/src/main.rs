use std::io::{Read};

use crate::driver::Driver;

mod driver;
mod init;
mod models;
mod handlers;
mod utils;
mod ride_manager;

/// Receives a port and creates driver actor
#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let port = args[1].parse::<u16>().unwrap();
    let initial_position : (i32, i32) = (args[2].parse::<i32>().unwrap(), args[3].parse::<i32>().unwrap());

    // hardcoded for now
    let drivers_ports : Vec<u16> = vec![6000, 6001];

    Driver::start(port, drivers_ports, initial_position).await?;

    Ok(())
}
