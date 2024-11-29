use crate::passenger::{Passenger};
use actix::{Actor, ActorFutureExt, Handler, Message, StreamHandler, WrapFuture};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use actix::prelude::*;
use serde::Serialize;
use crate::models::*;

mod passenger;
mod utils;
mod models;

pub const LEADER_PORT: u16 = 6003;

/// Recibe un puerto y un archivo de destinos(rides) y crea el actor Pasajero
#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let port = args[1].parse::<u16>().unwrap();
    let orders_path = &args[2];

    let rides = utils::get_rides(orders_path)?;

    let rides_vec: Vec<RideRequest> = rides.iter().map(|(_, coordinates)| coordinates.clone()).collect();
    Passenger::start(port, rides_vec).await?;
    Ok(())
}
