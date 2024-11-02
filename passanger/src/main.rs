use crate::passanger::{Passenger, TcpSender};
use actix::{Actor, ActorFutureExt, Handler, Message, StreamHandler, WrapFuture};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, split};
use tokio::net::{TcpStream};
use tokio_stream::wrappers::LinesStream;


mod passanger;
mod utils;

const LEADER_PORT: u16 = 6000;

/// Recibe un puerto y un archivo de destinos(rides) y crea el actor Pasajero
#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let port = args[1].parse::<u16>().unwrap();
    let orders_path = &args[2];

    let rides = utils::get_rides(orders_path)?;

    /// Lo hice igual que la catedra, debe haber una manera de pasarlo al archivo passanger

    let stream = TcpStream::connect(format!("127.0.0.1:{}", LEADER_PORT)).await?;
    let addr = Passenger::create(move |ctx| {
        let (read, write_half) = split(stream);
        Passenger::add_stream(LinesStream::new(BufReader::new(read).lines()), ctx);
        let write = Some(write_half);
        let addr_tcp = TcpSender::new(write).start();
        Passenger::new(port, addr_tcp)
    });

    // Send the coordinates to the passenger actor
    for (_, coordinates) in rides.iter() {
        let coordinates_aux = coordinates.clone();
        println!("Enviando mensaje: {:?}", coordinates_aux);
        addr.do_send(coordinates_aux);
    }

    Ok(())
}
