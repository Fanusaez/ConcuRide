use std::io;
use std::sync::{Arc, RwLock};
use actix::{Actor, Context, StreamHandler};
use tokio::io::{split, AsyncBufReadExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio_stream::wrappers::LinesStream;


const LIDER_PORT_IDX : usize = 0;

pub struct Driver {
    /// The port of the driver
    pub id: u16,
    /// Whether the driver is the leader
    pub is_leader: Arc<RwLock<bool>>,
    // The connections to the drivers TODO
    //pub drivers_connections: Arc<HashMap<u16, TcpStream>>,
}


impl Actor for Driver {
    type Context = Context<Self>;
}

/// Handles the messages coming from the associated stream.
impl StreamHandler<Result<String, io::Error>> for Driver {
    fn handle(&mut self, read: Result<String, io::Error>, ctx: &mut Self::Context) {
        if let Ok(line) = read {
            println!("{}", line);
        } else {
            println!("[{:?}] Failed to read line {:?}", self.id, read);
        }
    }
}


impl Driver {
    /// Creates the actor and starts listening for incoming passengers
    /// # Arguments
    /// * `port` - The port of the driver
    /// * `drivers_ports` - The list of driver ports TODO (leader should try to connect to them)
    pub async fn start(port: u16, drivers_ports: Vec<u16>) -> Result<(), io::Error> {
        let should_be_leader = port == drivers_ports[LIDER_PORT_IDX];
        let is_leader = Arc::new(RwLock::new(should_be_leader));

        let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).await?;
        println!("WAITING FOR PASSENGERS TO CONNECT\n");

        while let Ok((stream,  _)) = listener.accept().await {

            println!("CONNECTION ACCEPTED\n");

             Driver::create(|ctx| {
                let (read, _write_half) = split(stream);
                Driver::add_stream(LinesStream::new(BufReader::new(read).lines()), ctx);
                //let write = Some(write_half);
                Driver {
                    id: port,
                    is_leader: is_leader.clone(),
                }
            });
        }
        Ok(())
    }
}