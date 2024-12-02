//! # Mi Crate
use crate::models::*;
use crate::passenger::Passenger;

/// Passenger module
mod passenger;
/// Utils module
mod utils;
/// Models module
mod models;

/// Initial Leader Port
pub const LEADER_PORT: u16 = 6000;

/// Recibe un puerto y un archivo de destinos(rides) y crea el actor Pasajero
#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let port = args[1].parse::<u16>().unwrap();
    let orders_path = &args[2];
    let rides = utils::get_rides(orders_path)?;
    let rides_vec: Vec<RideRequest> = rides.values().cloned().collect();
    Passenger::start(port, rides_vec).await?;
    Ok(())
}
