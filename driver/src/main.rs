use std::io::{Read};

use crate::driver::Driver;
use crate::utils::read_config_file;

mod driver;
mod init;
mod models;
mod handlers;
mod utils;
mod ride_manager;

const CONFIG_PATH: &str = "config.txt";

/// Receives a port and creates driver actor
#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let port = args[1].parse::<u16>().unwrap();
    let initial_position : (i32, i32) = (args[2].parse::<i32>().unwrap(), args[3].parse::<i32>().unwrap());

    // read from config file
    let drivers_ports = read_config_file(CONFIG_PATH)?;

    Driver::start(port, drivers_ports, initial_position).await?;

    Ok(())
}
