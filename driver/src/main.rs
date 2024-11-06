use std::io::{Read};
use actix_rt::System;
use crate::driver::Driver;

mod driver;

#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let port = args[1].parse::<u16>().unwrap();

    // hardcoded for now
    let drivers_ports : Vec<u16> = vec![6000, 6001, 6002, 6003, 6004];

    Driver::start(port, drivers_ports).await?;

    tokio::signal::ctrl_c().await.expect("Error al esperar Ctrl+C");
    System::current().stop();

    Ok(())
}
