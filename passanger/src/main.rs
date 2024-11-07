use crate::passenger::{Passenger};
use actix::{Actor, ActorFutureExt, Handler, Message, StreamHandler, WrapFuture};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use actix::prelude::*;
use serde::Serialize;
use tokio::sync::oneshot;
use crate::utils::Coordinates;

mod passenger;
mod utils;

const LEADER_PORT: u16 = 6000;

/// Recibe un puerto y un archivo de destinos(rides) y crea el actor Pasajero
#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let port = args[1].parse::<u16>().unwrap();
    let orders_path = &args[2];

    let rides = utils::get_rides(orders_path)?;

    let rides_vec: Vec<Coordinates> = rides.iter().map(|(_, coordinates)| coordinates.clone()).collect();
    let (tx, rx) = oneshot::channel();
    Passenger::start(port, rides_vec, tx).await?;
    let _ = rx.await;

    Ok(())
}
