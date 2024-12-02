use crate::payment::{log, PaymentApp};
use tokio::io::{split};
use actix::{Actor};
use tokio::net::TcpListener;

mod payment;
mod messages;

use crate::payment::{SocketWriter, SocketReader};

/// Default port used by the payment app to listen to new connections
const DEFAULT_PORT: u16 = 7500;

/// Main function. Creates the payment app, conformed by the actors
/// 'SocketReader', 'SocketWriter', and 'PaymentApp'
#[actix_rt::main]
async fn main() -> std::io::Result<()> {

    let listener = TcpListener::bind(format!("127.0.0.1:{}", DEFAULT_PORT)).await?;

    log(&format!("PAYMENT APP LISTENING ON PORT {}", DEFAULT_PORT), "INFO");

    while let Ok((stream, _)) = listener.accept().await {
        let (read_half, write_half) = split(stream);
        payment::log("NEW CLIENT CONNECTED", "NEW_CONNECTION");
        // Creation of SocketWriter actor
        let writer_addr = SocketWriter::new(Some(write_half)).start();

        // Creation of PaymentApp actor
        let payment_app_addr = PaymentApp::new(writer_addr).start();
    
        // Creation of SocketReader actor
        SocketReader::start(read_half, payment_app_addr).await?;
    }

    Ok(())
}