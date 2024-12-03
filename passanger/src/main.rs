//! Creates a passenger to connect to the leader driver
use crate::models::*;
use crate::passenger::Passenger;

/// Models module
mod models;
/// Passenger module
mod passenger;
/// Utils module
mod utils;

/// Initial Leader Port
pub const LEADER_PORT: u16 = 6000;

/// Receives a port and destinations file (rides) and creates the Passanger actor
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
