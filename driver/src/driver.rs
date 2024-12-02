use std::cmp::PartialEq;
use std::collections::HashMap;
use std::{io, thread};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::Duration;
use actix::{Actor, Addr, AsyncContext, StreamHandler, Context, WrapFuture};
use tokio::io::{split, AsyncBufReadExt, BufReader, AsyncWriteExt, WriteHalf, ReadHalf};
use tokio::net::{TcpListener, TcpStream};
use std::net::{UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio_stream::wrappers::LinesStream;
use actix::prelude::*;

use crate::{init};
use crate::models::*;
use crate::utils::*;
use crate::ride_manager::*;
use std::time::Instant;
use log::{debug, info};

pub struct LastPingManager {
    pub last_ping: Instant,
}

pub enum States {
    Driving,
    Idle,
    LeaderLess
}

impl PartialEq for States{
    fn eq(&self, other: &Self) -> bool {
        matches!((self, other), (States::Driving, States::Driving) | (States::Idle, States::Idle))
    }
}

#[derive(Clone, Debug)]
pub struct DriverStatus {
    pub last_response: Option<Instant>, // Último tiempo de respuesta
    pub is_alive: bool, // Estado del driver
}


const LIDER_PORT_IDX : usize = 0;
const PING_INTERVAL: u64 = 5;
const RESTART_DRIVER_SEARCH_INTERVAL: u64 = 5;
const PROBABILITY_OF_ACCEPTING_RIDE: f64 = 0.99;
const NO_DRIVER_AVAILABLE: u16 = 0;
const PING_TIMEOUT: u64 = 7;
const TIMEOUT: Duration = Duration::from_secs(5);
const CHECK_LEADER_TIME: u64 = 5;

pub type FullStream = (Option<ReadHalf<TcpStream>>, Option<WriteHalf<TcpStream>>);

pub struct Driver {
    /// The id of the driver, for listening connections and identifying the driver
    pub id: u16,
    /// The driver's position
    pub position: (i32, i32),
    /// Whether the driver is the leader
    pub is_leader: Arc<AtomicBool>,
    /// Leader port
    pub leader_port: u16,
    /// write half
    pub write_half_to_leader: Option<WriteHalf<TcpStream>>,
    /// Liders last ping
    pub last_ping_by_leader: Addr<LastPingManager>,
    /// The connections to the drivers
    pub active_drivers: HashMap<u16, FullStream>,
    /// ID's of Dead Drivers
    pub dead_drivers: Vec<u16>,
    /// State of the driver
    pub state: States,
    /// Last known position of the driver (id, (x, y))
    pub drivers_last_position: HashMap<u16, (i32, i32)>,
    /// Connection to the payment app
    pub payment_write_half: Option<WriteHalf<TcpStream>>,
    /// Ride manager, contains the pending rides, unpaid rides and the rides and offers
    pub ride_manager: RideManager,
    /// ID's and WriteHalf of passengers
    pub passengers_write_half: HashMap<u16, Option<WriteHalf<TcpStream>>>,
    /// Status of the drivers, contains the last response and if the driver is alive
    pub drivers_status:HashMap<u16, DriverStatus>,
    /// UDP Socket for leader election
    pub socket: Arc<UdpSocket>,
    /// Driver's IDS
    pub drivers_id: Vec<u16>,
    /// All known passengers id's
    pub passengers_id: Vec<u16>,
}

impl Driver {

    /// Creates the actor and starts listening for incoming passengers
    /// # Arguments
    /// * `port` - The port of the driver
    /// * `drivers_ports` - The list of driver ports TODO (leader should try to connect to them)
    pub async fn start(port: u16, mut drivers_ports: Vec<u16>, position: (i32, i32)) -> Result<(), io::Error> {
        // Driver-leader attributes
        let should_be_leader = port == drivers_ports[LIDER_PORT_IDX];
        let is_leader = should_be_leader;
        let is_leader_arc = Arc::new(AtomicBool::new(is_leader));
        let leader_port = drivers_ports[LIDER_PORT_IDX];
        let last_ping_manager = LastPingManager { last_ping: Instant::now() }.start();
        // todo: ver para no hardcodear esto
        let passengers_id = vec![9000];

        // Auxiliar structures
        let mut active_drivers: HashMap<u16, FullStream> = HashMap::new();
        let mut drivers_last_position: HashMap<u16, (i32, i32)> = HashMap::new();
        let  passengers_write_half: HashMap<u16, Option<WriteHalf<TcpStream>>> = HashMap::new();
        let mut drivers_status: HashMap<u16, DriverStatus> = HashMap::new();

        // Payment app and connection
        let mut payment_write_half: Option<WriteHalf<TcpStream>> = None;
        let mut payment_read_half: Option<ReadHalf<TcpStream>> = None;

        // Leader election (tengo pensado usar los puertos 10001, 10002, ...)
        let port_socket = port + 4000;
        let socket = UdpSocket::bind(format!("127.0.0.1:{}", port_socket))?;

        // Remove the leader port from the list of drivers
        drivers_ports.remove(LIDER_PORT_IDX);

        let drivers_id = drivers_ports.clone();

        init::init_driver(&mut active_drivers, drivers_ports,
                          &mut drivers_last_position,
                          should_be_leader,
                          &mut payment_write_half,
                          &mut payment_read_half,
                          &mut drivers_status).await?;

        // para que funcione, no se porque
        let associate_driver_streams = Self::associate_drivers_stream;
        let associate_payment_stream = Self::associate_payment_stream;
        let handle_passenger_connection = Self::handle_passenger_connection;
        let handle_leader_connection = Self::handle_leader_connection;


        let driver = Driver::create(|ctx| {
            // Associates the drivers and payment streams to the leader
            if should_be_leader {

                if let Err(e) = associate_driver_streams(ctx, &mut active_drivers) {
                    debug!("Error al asociar los streams de los drivers: {:?}", e);
                }
                if let Err(e) = associate_payment_stream(ctx, payment_read_half) {
                    debug!("Error al asociar los streams de los pagos: {:?}", e);
                }

            }
            Driver {
                id: port,
                position,
                is_leader: is_leader_arc.clone(),
                leader_port,
                last_ping_by_leader: last_ping_manager,
                active_drivers,
                dead_drivers: Vec::new(),
                write_half_to_leader: None,
                passengers_write_half,
                state: States::Idle,
                drivers_last_position,
                payment_write_half,
                drivers_status,
                ride_manager: RideManager::new(),
                socket: Arc::new(socket),
                drivers_id,
                passengers_id,

            }
        });

        let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).await?;

        while let Ok((stream, addr)) = listener.accept().await {
            if is_leader_arc.load(Ordering::SeqCst) {
                handle_passenger_connection(stream, addr.port(), &driver)?;
            }
            else {
                handle_leader_connection(stream, &driver)?;
            }
        }
        Ok(())
    }


    /// Associates the drivers streams to the driver actor, only used by the leader
    /// # Arguments
    /// * `ctx` - The context of the driver
    /// * `active_drivers` - The id and the full stream to the drivers
    pub fn associate_drivers_stream(
        ctx: &mut Context<Driver>,
        active_drivers: &mut HashMap<u16, FullStream>)
        -> Result<(), io::Error>
    {
        for (id, (read, _)) in active_drivers.iter_mut() {
            if let Some(read_half) = read.take() {
                Driver::add_stream(LinesStream::new(BufReader::new(read_half).lines()), ctx);
            } else {
                debug!("No read stream available for driver {}", id);
            }
        }
        Ok(())
    }

    /// Associates the payment stream to the driver actor, only used by the leader
    /// # Arguments
    /// * `ctx` - The context of the driver
    /// * `payment_read_half` - The arc of the payment read half
    pub fn associate_payment_stream(
        ctx: &mut Context<Driver>,
        mut payment_read_half: Option<ReadHalf<TcpStream>>)
        -> Result<(), io::Error> {

        if let Some(payment_read_half) = payment_read_half.take() {
            Driver::add_stream(LinesStream::new(BufReader::new(payment_read_half).lines()), ctx);
        } else {
            debug!("No read stream available for payment");
        }
        Ok(())
    }

    /// Manages the connection of a passenger, only used by the leader
    /// # Arguments
    /// * `stream` - The stream of the passenger
    /// * `id` - The id of the passenger
    /// * `driver` - The address of the driver
    fn handle_passenger_connection(
        stream: TcpStream,
        port_used: u16,
        driver: &Addr<Driver>,
    ) -> Result<(), io::Error> {
        let (read, write) = split(stream);

        if driver.connected() {
            // Sends the read stream to the driver
            match driver.try_send(StreamMessage { stream: Some(read) }) {
                Ok(_) => {}
                Err(e) => debug!("Error sending the read stream to the actor: {:?}", e),
            }
            // Sends the write stream + the id of the passenger to the driver
            match driver.try_send(NewPassengerHalfWrite { passenger_id: port_used, write_half: Some(write) }) {
                Ok(_) => {}
                Err(e) => debug!("Error sending : {:?}", e),
            }

        } else {
            debug!("The actor is not connected");
        }
        Ok(())
    }

    /// Manages the connection of a leader, only used by the drivers
    /// # Arguments
    /// * `stream` - The stream of the leader
    /// * `half_write_to_leader` - The arc of the write half to the leader
    /// * `driver` - The address of the driver
    fn handle_leader_connection(
        stream: TcpStream,
        addr: &Addr<Driver>,
    ) -> Result<(), io::Error> {
        let (read, write) = split(stream);

        // Sends the read stream to the driver
        match addr.try_send(StreamMessage { stream: Some(read) }) {
            Ok(_) => {}
            Err(e) => {
                debug!("Error sending the read stream to the actor: {:?}", e);
            }
        }

        // Sends the write stream of the leader to the driver
        match addr.try_send(WriteHalfLeader { write_half: Some(write) }) {
            Ok(_) => {}
            Err(e) => {
                debug!("Error sending the write stream of the leader to the actor: {:?}", e);
            }
        }

        Ok(())
    }

    /// Handles the stream message of the leader, only used by the drivers
    /// # Arguments
    /// * `msg` - The message containing the stream of the leader
    /// * `ctx` - The context of the driver
    pub fn handle_write_half_leader_as_driver(&mut self, msg: WriteHalfLeader) -> Result<(), io::Error> {
        if let Some(write_half) = msg.write_half {
            self.write_half_to_leader = Some(write_half);
        } else {
            debug!("No write stream available for the leader");
        }
        Ok(())
    }

/// ------------------------------------------------------------------Fin del start/inicializacion -------------------------------------------- ///


/// ------------------------------------------------------------------ PING IMPLEMENTATION ---------------------------------------------------- ///


    /// Starts the ping system, only used by leader
    /// In a loop sends pings to the drivers
    /// # Arguments
    /// * `ctx` - The context of the driver
    pub fn start_ping_system(&mut self, addr: Addr<Driver>, ctx: &mut Context<Self>) {
        ctx.run_interval(Duration::from_secs(PING_INTERVAL), move |actor, _ctx| {
            // For each driver, send a ping
            let driver_ids: Vec<u16> = actor.drivers_status.keys().cloned().collect();

            for driver_id in driver_ids {
                addr.do_send(SendPingTo { id_to_send: driver_id });

                // Check if the driver is alive
                if !actor.update_driver_status(driver_id) {
                    addr.do_send(DeadDriver { driver_id });
                }
            }
        });
    }

    /// Handles the ping message as a leader
    /// This is a response Ping from a driver (PONG)
    /// # Arguments
    /// * `msg` - The message containing the ping
    pub fn handle_ping_as_leader(&mut self, msg: Ping) -> Result<(), io::Error> {
        let driver_id = msg.id_sender;
        // Update last ping time of driver
        self.drivers_status.get_mut(&driver_id).unwrap().last_response = Some(Instant::now());
        Ok(())
    }

    /// Handles the ping message as a driver, only used by the driver
    /// It will send a pong message to the leader
    /// # Arguments
    /// * `msg` - The message containing the ping
    /// * `ctx` - The context of the driver
    pub fn handle_ping_as_driver(&mut self, msg: Ping, ctx: &mut Context<Self>) -> Result<(), io::Error> {
        let response = Ping {
            id_sender: self.id,
            id_receiver: msg.id_sender,
        };
        // Update last ping time of leader
        self.last_ping_by_leader.do_send(UpdateLastPing { time: Instant::now() });

        // Resend ping (PONG)
        self.send_message_to_leader(MessageType::Ping(response), ctx)?;
        Ok(())
    }

    /// Sends a ping to the driver with the specified id, only used by the leader
    /// # Arguments
    /// * `id_driver` - The id of the driver
    /// * `ctx` - The context of the driver
    pub fn send_ping_to_driver(&mut self, id_driver: u16, ctx: &mut Context<Self>) -> Result<(), io::Error> {
        let message = Ping {
            id_sender: self.id,
            id_receiver: id_driver,
        };
        self.send_message_to_driver(id_driver, MessageType::Ping(message), ctx)?;
        Ok(())
    }

    /// Checks if the leader is alive, only used by driver
    /// if the leader stop sending pings, the driver will send an auto message
    /// # Arguments
    /// * `addr` - The address of the driver
    pub fn check_leader_alive(&mut self, addr: Addr<Driver>) {
        let last_ping_by_leader = self.last_ping_by_leader.clone();
        let leader_id = self.leader_port;

        actix::spawn(async move {
            loop {
                let last_ping = last_ping_by_leader.send(GetLastPing).await.unwrap();

                let now = Instant::now();
                let elapsed = now.duration_since(last_ping).as_secs();

                if elapsed > PING_TIMEOUT {
                    addr.do_send(DeadLeader { leader_id });
                    break;
                }

                tokio::time::sleep(Duration::from_secs(PING_INTERVAL)).await;
            }
        });
    }


    /// ------------------------------------------------------------------ END PING IMPLEMENTATION ---------------------------------------------------- ///

    /// Handles the ride request from the leader as a driver
    /// # Arguments
    /// * `msg` - The message containing the ride request
    /// * `ctx` - The context of the driver
    pub fn handle_ride_request_as_driver(&mut self, msg: RideRequest, addr: Addr<Self>, ctx: &mut Context<Self>) -> Result<(), io::Error> {
        let result = boolean_with_probability(PROBABILITY_OF_ACCEPTING_RIDE);

        if result && self.state == States::Idle {
            log(&format!("RIDE REQUEST {} ACCEPTED", msg.id), "DRIVER");
            self.accept_ride_request(msg, ctx)?;
            self.drive_and_finish(msg, addr)?;
        } else {
            log(&format!("RIDE REQUEST {} REJECTED", msg.id), "DRIVER");
            self.decline_ride_request(msg, ctx)?;
        }
        Ok(())

    }

    /// Handles the ride request from passenger, sends RideRequest to the closest driver
    /// # Arguments
    /// * `msg` - The message containing the ride request
    /// * `ctx` - The context of the driver
    pub fn handle_ride_request_as_leader(&mut self, msg: RideRequest, ctx: &mut Context<Self>) -> Result<(), io::Error> {

        // Ignore request if there is a pending ride request
        if self.ride_manager.has_pending_ride_request(msg.id) {
            return Ok(());
        }

        // Insert the ride request in the unpaid rides
        self.ride_manager.insert_unpaid_ride(msg).expect("Error adding unpaid ride");

        // Send the payment to the payment app
        self.send_payment(msg, ctx)?;

        Ok(())
    }

    /// Handles the payment accepted message from the payment app
    /// Sends the ride request to the closest driver and adds the driver to the offers
    /// # Arguments
    /// * `msg` - The message containing the PaymentAccepted
    /// * `ctx` - The context of the driver
    pub fn handle_payment_accepted_as_leader(&mut self, msg: PaymentAccepted, addr: Addr<Self>, ctx: &mut Context<Self>) -> Result<(), io::Error> {
        // Remove the ride request from the unpaid rides
        let ride_request = self.ride_manager.remove_unpaid_ride(msg.id)?;

        // Saves ride in pending_rides
        self.ride_manager.insert_ride_in_pending(ride_request)?;

        // Search the closest driver to the passenger and send the ride request
        self.search_driver_and_send_ride(ride_request, addr, ctx)?;

        // Insert the ride in paid rides
        self.ride_manager.insert_ride_in_paid_rides(msg.id, msg)?;

        Ok(())
    }

    /// Search the closest driver to the passenger and send the ride request
    /// Only used by the leader
    /// Insert the driver id in the ride_and_offers hashmap
    /// # Arguments
    /// * `ride_request` - The ride request to send
    /// * `ctx` - The context of the driver
    fn search_driver_and_send_ride(&mut self, ride_request: RideRequest, addr: Addr<Self>, ctx: &mut Context<Self>) -> Result<(), io::Error> {
        // Closest driver to the passenger
        let driver_id_to_send = self.get_closest_driver(ride_request);

        // No driver available
        if driver_id_to_send == NO_DRIVER_AVAILABLE {
            log(&format!("NO DRIVERS AVAILABLE RIGHT NOW FOR PASSENGER {}, TRYING AGAIN LATER", ride_request.id), "INFO");

            // Delete all the offers for the ride, so it can be offered again
            self.ride_manager.remove_offers_from_ride_and_offers(ride_request.id)?;

            // Pause and restart the driver search
            self.pause_and_restart_driver_search(ride_request, addr)?;

            return Ok(());
        }

        // Send the ride request to the driver
        self.send_message_to_driver(driver_id_to_send, MessageType::RideRequest(ride_request), ctx)?;

        // Insert the driver id in the ride_and_offers hashmap
        self.ride_manager.insert_in_rides_and_offers(ride_request.id, driver_id_to_send)?;

        Ok(())
    }

    /// Pause and restart the driver search, so we dont do a busy wait
    /// Only used by the leader
    /// # Arguments
    /// * `ride_request` - The ride request to send
    /// * `addr` - The address of the driver
    fn pause_and_restart_driver_search(&mut self, ride_request: RideRequest, addr: Addr<Self>) -> Result<(), io::Error> {

        actix::spawn(async move {
            // Sleep for a while and then restart the driver search
            tokio::time::sleep(Duration::from_secs(RESTART_DRIVER_SEARCH_INTERVAL)).await;

            let response = RestartDriverSearch {
                passenger_id: ride_request.id,
            };
            // Restart the driver search
            addr.do_send(response);
        });
        Ok(())
    }

    /// Handles the payment rejected message from the payment app
    /// Only used by the leader
    /// Sends the payment rejected message to the passenger
    pub fn handle_payment_rejected_as_leader(&mut self, msg: PaymentRejected, ctx: &mut Context<Self>) -> Result<(), io::Error> {
        self.send_message_to_passenger(MessageType::PaymentRejected(msg), msg.id, ctx)?;
        Ok(())
    }

    /// Handles the accept ride message from the driver
    /// Removes the passenger id from ride_and_offers
    /// # Arguments
    /// * `msg` - The message containing the AcceptRide
    pub fn handle_accept_ride_as_leader(&mut self, msg: AcceptRide) -> Result<(), io::Error> {
        // Remove the passenger ID from ride_and_offers in case the passenger wants to take another ride
        if let Err(e) = self.ride_manager.remove_from_ride_and_offers(msg.passenger_id) {
            debug!(
                "Error removing passenger ID {} from ride_and_offers: {:?}",
                msg.passenger_id, e
            );
        }
        Ok(())
    }

    /// Handles the declined ride from driver as leader
    /// Sends the ride request to the next closest driver and adds the driver to the offers
    /// # Arguments
    /// * `msg` - The message containing the Declined Ride
    /// * `ctx` - The context of the driver
    pub fn handle_declined_ride_as_leader(&mut self, msg: DeclineRide, addr: Addr<Self>, ctx: &mut Context<Self>) -> Result<(), io::Error> {
        let ride_request = self.ride_manager.get_pending_ride_request(msg.passenger_id)?;

        // Search for another driver and send the ride request
        self.search_driver_and_send_ride(ride_request, addr, ctx)?;

        Ok(())
    }

    /// Handles the RestartDriverSearch message, only used by the leader
    /// Restarts the driver search for the passenger
    /// # Arguments
    /// * `msg` - The message containing the RestartDriverSearch
    /// * `ctx` - The context of the driver
    pub fn handle_restart_driver_search_as_leader(&mut self, msg: RestartDriverSearch, addr: Addr<Self>, ctx: &mut Context<Self>) -> Result<(), io::Error> {
        let ride_request = self.ride_manager.get_pending_ride_request(msg.passenger_id)?;

        // Search for another driver and send the ride request
        // todo: esto es igual a lo de arruba, deberiamos borrar este mensaje y que se envie un declined ride en pause_And_restart...
        self.search_driver_and_send_ride(ride_request, addr, ctx)?;

        Ok(())
    }

    /// Handles the FinishRide message from the driver
    /// Removes the ride from the pending rides and sends the FinishRide message to the passenger
    /// # Arguments
    /// * `msg` - The message containing the FinishRide
    /// * `ctx` - The context of the driver
    pub fn handle_finish_ride_as_leader(&mut self, msg: FinishRide, ctx: &mut Context<Self>) -> Result<(), io::Error> {
        // Remove the ride from the pending rides
        // todo: ver manejo de errores, la funcion esta la hardcodee para que no devuelva un error
        self.ride_manager.remove_ride_from_pending(msg.passenger_id)?;

        let msg_message_type = MessageType::FinishRide(msg);

        // Send the FinishRide message to the passenger
        self.send_message_to_passenger(msg_message_type, msg.passenger_id, ctx)?;

        // Pay ride to driver
        // todo: en caso de ser un lider nuevo, no va a encontrar el pago, que hago en este caso?
        match self.ride_manager.get_ride_from_paid_rides(msg.passenger_id) {
            Ok(payment) => {
                let pay_ride_msg = PayRide { ride_id: msg.passenger_id, amount: payment.amount };
                self.send_payment_to_driver(msg.driver_id, pay_ride_msg, ctx)?;
            }
            // todo: no encuentra porque se cambio de lider (esta bien asi?)
            Err(e) => debug!("Error getting payment from paid rides: {:?}", e),
        }

        Ok(())
    }

    /// Sends the payment message with the money to the driver
    /// Only used by the leader
    /// # Arguments
    /// * `msg` - The message containing the payment
    /// * `ctx` - The context of the driver
    pub fn send_payment_to_driver(&mut self, driver_id: u16, msg: PayRide, ctx: &mut Context<Self>) -> Result<(), io::Error> {
        self.send_message_to_driver(driver_id, MessageType::PayRide(msg), ctx)
    }

    /// Finish the ride, send the FinishRide message to the leader
    /// # Arguments
    /// * `msg` - The message containing the ride request
    pub fn handle_finish_ride_as_driver(&mut self, msg: FinishRide, addr: Addr<Driver>, ctx: &mut Context<Self>) -> Result<(), io::Error> {
        log(&format!("RIDE FINISHED, PASSENGER WITH ID {} HAS BEEN DROPPED OFF", msg.passenger_id), "DRIVER");

        // Cambiar el estado del driver a Idle
        if self.state != States::LeaderLess {
            self.state = States::Idle;
            self.send_message_to_leader(MessageType::FinishRide(msg), ctx)?;
        }
        // If the driver is leaderless, it will wait until it has one to send the FinisgRide message
        else {
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(CHECK_LEADER_TIME)).await;
                addr.do_send(msg);
            });
        }
        Ok(())
    }

    /// Handles the dead driver message from the leader
    /// Removes the driver from the active drivers and adds it to the dead drivers
    /// In case the dead driver was offered a ride and not responded, sends auto decline message, so the RideRequest can be sent to another driver
    ///
    /// # Arguments
    /// * `addr` - The address of the driver
    /// * `msg` - The message containing the dead driver
    pub fn handle_dead_driver_as_leader(&mut self, addr: Addr<Self>, msg: DeadDriver) -> Result<(), io::Error> {

        let dead_driver_id = msg.driver_id;

        // Remove the driver from the active drivers
        self.active_drivers.remove(&dead_driver_id);
        // Add the driver id to the dead drivers
        self.dead_drivers.push(dead_driver_id);
        // Remove the driver from the drivers last position
        self.drivers_last_position.remove(&dead_driver_id);

        // Cover case where the driver was offered a ride and did not respond
        for (passenger_id, drivers_id) in self.ride_manager.ride_and_offers.iter_mut() {
            if let Some(last_driver) = drivers_id.last() {
                // If the last driver was the dead driver, send a decline message
                if *last_driver == dead_driver_id {
                    addr.do_send(DeclineRide { passenger_id: *passenger_id, driver_id: dead_driver_id });
                }
            }
        }
        Ok(())
    }

    /// Driver's function
    /// Sends the AcceptRide message to the leader
    /// # Arguments
    /// * `msg` - The message containing the ride request
    /// * `ctx` - The context of the driver
    fn accept_ride_request(&mut self, ride_request_msg: RideRequest, ctx: &mut Context<Self>) -> Result<(), io::Error> {

        // Change the state of the driver to Driving
        self.state = States::Driving;

        // AcceptRide message
        let response = AcceptRide {
            passenger_id: ride_request_msg.id,
            driver_id: self.id,
        };

        // Send the AcceptRide message to the leader
        self.send_message_to_leader(MessageType::AcceptRide(response), ctx)?;

        Ok(())
    }

    /// Driver's function
    /// Sends the DeclineRide message to the leader
    /// # Arguments
    /// * `msg` - The message containing the ride request
    fn decline_ride_request(&mut self, msg: RideRequest, ctx: &mut Context<Self>) -> Result<(), io::Error> {

        // Crear el mensaje de respuesta
        let response = DeclineRide {
            passenger_id: msg.id,
            driver_id: self.id,
        };

        // Enviar el mensaje de manera asíncrona
        self.send_message_to_leader(MessageType::DeclineRide(response), ctx)?;

        Ok(())
    }

    /// Driver's function
    /// Simulates the travel from the origin to the destination
    /// # Arguments
    /// * `msg` - The message containing the ride request
    /// * `addr` - The address of the driver
    fn drive_and_finish(&mut self, msg: RideRequest, addr: Addr<Self>) -> Result<(), io::Error> {
        let driver_id = self.id;
        // todo: ver duracion, lo hardcodee abajo
        let _duration = calculate_travel_duration(&msg);

        actix::spawn(async move {
            let mut current_position = (msg.x_origin, msg.y_origin);
            let advancement = 10;  // Dividir el viaje en 10 pasos
            let x_km = (msg.x_dest as i32 - msg.x_origin as i32) as f64 / advancement as f64;
            let y_km = (msg.y_dest as i32 - msg.y_origin as i32) as f64 / advancement as f64;

            for i in 0..=advancement {
                current_position.0 = (msg.x_origin as f64 + x_km * i as f64) as u16;
                current_position.1 = (msg.y_origin as f64 + y_km * i as f64) as u16;

                let position_update = PositionUpdate {
                    driver_id,
                    position: (current_position.0 as i32, current_position.1 as i32),
                };

                addr.do_send(position_update);

                actix_rt::time::sleep(Duration::from_secs(2)).await;
            }

            let finish_ride = FinishRide {
                passenger_id: msg.id,
                driver_id,
            };

            addr.do_send(finish_ride);
        });

        Ok(())
    }

    /// Handles the PositionUpdate message from the driver
    /// Args:
    /// * `msg` - The message containing the position update
    pub fn handle_position_update_as_leader(&mut self, msg: PositionUpdate) -> Result<(), io::Error> {
        info!("UPDATING DRIVER {} POSITION TO {:?}", msg.driver_id, msg.position);
        self.drivers_last_position.insert(msg.driver_id, msg.position);
        Ok(())
    }

    /// Handles the PositionUpdate message, only used by the driver
    /// Sends the position update to the leader in case there is one
    /// Updates the driver's position
    /// # Arguments
    /// * `msg` - The message containing the position update
    /// * `ctx` - The context of the driver
    pub fn handle_position_update_as_driver(&mut self, msg: PositionUpdate, ctx: &mut Context<Self>) -> Result<(), io::Error> {
        self.position = msg.position;
        // If leaderless, do nothing. Else send the position update to the leader
        if self.state != States::LeaderLess {
            self.send_message_to_leader(MessageType::PositionUpdate(msg), ctx)?;
        }
        Ok(())
    }

    /// Sends message to the payment app containing the ride price to make a reservation
    /// Only used by the leader
    /// # Arguments
    /// * `msg` - The message containing the ride request
    /// * `ctx` - The context of the driver
    pub fn send_payment(&mut self, msg: RideRequest, ctx: &mut Context<Self>) -> Result<(), io::Error> {
        // Calculate the price of the ride based on distance
        let ride_price = calculate_price(msg);

        // Send the payment message to the payment app
        let message = SendPayment{id: msg.id, amount: ride_price};
        self.send_message_to_payment_app(MessageType::SendPayment(message), ctx)?;
        Ok(())
    }


    /// Checks if there is a pending ride request for the passenger
    /// If there is, sends a message to the passenger to reconnect
    /// Only used by the leader
    /// # Arguments
    /// * `passenger_id` - The id of the passenger
    /// * `ctx` - The context of the driver
    pub fn verify_pending_ride_request(&mut self, passenger_id: u16, ctx: &mut Context<Self>) -> Result<(), io::Error> {
        // Verify if the passenger has a pending ride request
        let has_pending = self.ride_manager.has_pending_ride_request(passenger_id);

        if !has_pending {
            return Ok(());
        }

        // Reconnect the passenger
        // todo: ver esto, no puede ser con un string
        let message = RideRequestReconnection {
            passenger_id,
            state: "WaitingDriver".to_string(),
        };

        self.send_message_to_passenger(MessageType::RideRequestReconnection(message), passenger_id, ctx)?;

        Ok(())
    }

    /// Handles the new leader message, this means that I am the new leader
    /// # Arguments
    /// * `msg` - The message containing the new leader
    /// * `addr` - The address of the driver
    pub fn handle_be_leader_as_driver(&mut self, msg: NewLeader, addr: Addr<Driver>) -> Result<(), io::Error> {
        // Change myself to leader
        self.is_leader.store(true, Ordering::SeqCst);
        // Change the state to idle
        self.state = States::Idle;
        // Set the leader port
        self.leader_port = self.id;
        // Sets the drivers id and removes myself from the list
        self.drivers_id = msg.drivers_id;
        self.drivers_id.retain(|&x| x != self.id);

        // Connect to the passengers as new leader
        self.connect_to_passengers_as_new_leader(addr.clone())?;

        let drivers_id = self.drivers_id.clone();
        tokio::spawn(async move {
            // todo: si hay tiempo hacer con ctx spawn, podrias hacer todo sin enviarte un mensaje con los datos
            Self::initialize_new_leader(drivers_id, addr).await;
        });
        Ok(())
    }

    /// Initializes the new leader
    /// Sets the attributes and establishes the connections to app payments and drivers
    /// # Arguments
    /// * `drivers_id` - The id of the drivers to connect
    /// * `addr` - The address of the driver
    async fn initialize_new_leader(drivers_id: Vec<u16>, addr: Addr<Driver>) {
        let mut active_drivers: HashMap<u16, FullStream> = HashMap::new();
        let mut drivers_last_position: HashMap<u16, (i32, i32)> = HashMap::new();
        let mut drivers_status: HashMap<u16, DriverStatus> = HashMap::new();

        // Payment app and connection
        let mut payment_write_half: Option<WriteHalf<TcpStream>> = None;
        let mut payment_read_half: Option<ReadHalf<TcpStream>> = None;

        init::init_driver(
            &mut active_drivers,
            drivers_id,
            &mut drivers_last_position,
            true,
            &mut payment_write_half,
            &mut payment_read_half,
            &mut drivers_status).await.expect("TODO: panic message");

        addr.do_send(NewLeaderAttributes {
            active_drivers,
            drivers_last_position,
            payment_write_half,
            payment_read_half,
            drivers_status,
        });

    }

    /// Handles the new leader attributes message, only used by the leader
    /// Sets the attributes of the new leader
    /// # Arguments
    /// * `msg` - The message containing the new leader attributes
    /// * `addr` - The address of the driver
    /// * `ctx` - The context of the driver
    pub fn handle_new_leader_attributes_as_leader(&mut self, msg: NewLeaderAttributes, addr: Addr<Driver>, ctx: &mut Context<Self>) -> Result<(), io::Error> {
        // Sets new attributes
        self.active_drivers = msg.active_drivers;
        self.drivers_last_position = msg.drivers_last_position;
        self.payment_write_half = msg.payment_write_half;
        self.drivers_status = msg.drivers_status;

        // Associate the read stream of the payment app
        Driver::add_stream(LinesStream::new(BufReader::new(msg.payment_read_half.unwrap()).lines()), ctx);

        // Associate the read streams of the drivers
        for (id, (read, _)) in self.active_drivers.iter_mut() {
            if let Some(read_half) = read.take() {
                Driver::add_stream(LinesStream::new(BufReader::new(read_half).lines()), ctx);
            } else {
                debug!("No read stream available for driver with {}", id);
            }
        }

        // Restart ping system
        self.start_ping_system(addr, ctx);

        Ok(())
    }

    /// Handles the new leader message, only used by the driver
    /// Informs the driver that there is a new leader
    /// # Arguments
    /// * `msg` - The message containing the new leader
    pub fn handle_new_leader_as_driver(&mut self, msg: NewLeader, addr: Addr<Driver>) -> Result<(), io::Error> {
        // todo: esta bien que pase a idle?. ESta mal, que sucede si estaba manejando un pasajero? EL nuevo lider le puede dar otro viaje, ver despues

        self.state = States::Idle;
        // Change the leader port
        self.leader_port = msg.leader_id;
        // New leader just connected
        self.last_ping_by_leader.do_send(UpdateLastPing { time: Instant::now() });
        // Restart the ping system
        self.check_leader_alive(addr);

        Ok(())
    }

    /// Connects to the passengers as new leader
    /// Only used by the leader
    /// # Arguments
    /// * `addr` - The address of the driver
    /// * `ctx` - The context of the driver
    // todo: podria hcerlo con ctx.spawn y eliminar el msaneje ese de NewPassengerConnection
    fn connect_to_passengers_as_new_leader(&self, addr: Addr<Driver>) -> Result<(), io::Error> {
        let passengers_id = self.passengers_id.clone();
        tokio::spawn(async move {
            for passenger_id in passengers_id {
                // puede pasar que el driver no este conectado
                let stream = match TcpStream::connect(format!("127.0.0.1:{}", passenger_id)).await {
                    Ok(stream) => stream,
                    Err(e) => {
                        debug!("Error al conectar con el pasajero {}: {:?}", passenger_id, e);
                        continue;
                    }
                };
                let (read, write) = split(stream);
                // envio read, write y id para agregarlo
                addr.do_send(NewPassengerConnection { passenger_id, read, write });
            }
        });

        Ok(())
    }

    /// Guarda stream de escritura de un pasajero en el hash y manda un mensaje para guardar el stream de lectura
    /// nombre se puede prestar a confusion, todo: cambiar despues
    pub fn handle_new_passenger_connection_as_leader(&mut self, msg: NewPassengerConnection, ctx: &mut Context<Self>) -> Result<(), io::Error> {
        // Saves the write stream of the passenger and the id
        self.passengers_write_half.insert(msg.passenger_id, Some(msg.write));
        // Associate the read stream of the passenger
        Driver::add_stream(LinesStream::new(BufReader::new(msg.read).lines()), ctx);
        Ok(())
    }

    /// ------------------------------------------- SENDING FUNCTIONS ---------------------------------------------- ///

    /// Driver's function
    /// Sends a message to the leader
    /// # Arguments
    /// * `message` - The message to send
    pub fn send_message_to_leader(&mut self, message: MessageType, ctx: &mut Context<Self>) -> Result<(), io::Error> {
        if let Some(write_half) = self.write_half_to_leader.take() {
            let serialized = serde_json::to_string(&message)?;

            ctx.spawn(
                async move {
                    let mut write_half = write_half;
                    if let Err(e) = write_half.write_all(format!("{}\n", serialized).as_bytes()).await {
                        log::debug!("Error al enviar el mensaje al líder: {}", e);
                    }

                    // Devolver el `write_half` al actor
                    write_half
                }
                    .into_actor(self) // Hace que el futuro corra dentro del contexto del actor
                    .map(|write_half, actor, _ctx| {
                        // Reinsertar `write_half` en el actor
                        actor.write_half_to_leader = Some(write_half);
                    }),
            );
        } else {
            debug!("No se pudo enviar el mensaje: no hay conexión activa");
        }
        Ok(())
    }


    /// TODO: hacer una funcion generica que reciba mensaje y canal de escritura para no repetir codigo
    fn send_message_to_payment_app(&mut self, message: MessageType, ctx: &mut Context<Self>) -> Result<(), io::Error> {
        if let Some(write_half) = self.payment_write_half.take() {
            let serialized = serde_json::to_string(&message)?;

            ctx.spawn(
                async move {
                    let mut write_half = write_half;
                    if let Err(e) = write_half.write_all(format!("{}\n", serialized).as_bytes()).await {
                        debug!("Error al enviar el mensaje al líder: {}", e);
                    }

                    // Devolver el `write_half` al actor
                    write_half
                }
                    .into_actor(self) // Hace que el futuro corra dentro del contexto del actor
                    .map(|write_half, actor, _ctx| {
                        // Reinsertar `write_half` en el actor
                        actor.payment_write_half = Some(write_half);
                    }),
            );
        } else {
            debug!("No se pudo enviar el mensaje: no hay conexión activa");
        }
        Ok(())
    }

    /// Generic function to send a message to the passenger specified by the id, only used by the leader
    /// # Arguments
    /// * `message` - The message to send
    /// * `passenger_id` - The id of the passenger
    pub fn send_message_to_passenger(
        &mut self,
        message: MessageType,
        passenger_id: u16,
        ctx: &mut Context<Self>,
    ) -> Result<(), io::Error> {
        if let Some(write_half) = self.passengers_write_half.get_mut(&passenger_id).and_then(|wh| wh.take()) {
            let serialized = serde_json::to_string(&message)?;

            ctx.spawn(
                async move {
                    let mut write_half = write_half;
                    if let Err(e) = write_half.write_all(format!("{}\n", serialized).as_bytes()).await {
                        log::debug!("Error al enviar el mensaje al pasajero {}: {:?}", passenger_id, e);
                    }

                    // Devolver el `write_half` al actor
                    (passenger_id, write_half)
                }
                    .into_actor(self)
                    .map(|(passenger_id, write_half), actor, _ctx| {
                        // Reinsertar el `write_half` en el mapa de `passengers_write_half`
                        if let Some(passenger_write_half) = actor.passengers_write_half.get_mut(&passenger_id) {
                            *passenger_write_half = Some(write_half);
                        }
                    }),
            );
        } else {
            debug!(
            "No se pudo enviar el mensaje: no hay conexión activa para el pasajero {}",
            passenger_id
        );
        }

        Ok(())
    }


    /// Sends the ride request to the driver specified by the id, only used by the leader
    /// # Arguments
    /// * `driver_id` - The id of the driver
    /// * `msg` - The message containing the ride request
    pub fn send_message_to_driver(
        &mut self,
        driver_id: u16,
        msg: MessageType,
        ctx: &mut Context<Self>,
    ) -> Result<(), io::Error> {
        if let Some((_, write_half)) = self.active_drivers.get_mut(&driver_id) {
            if let Some(write_half) = write_half.take() {
                let serialized = serde_json::to_string(&msg)?;

                ctx.spawn(
                    async move {
                        let mut write_half = write_half;
                        if let Err(e) = write_half.write_all(format!("{}\n", serialized).as_bytes()).await {
                            debug!("Error al enviar el mensaje al driver {}: {:?}", driver_id, e);
                        }

                        // Devolver el `write_half` al actor
                        (driver_id, write_half)
                    }
                        .into_actor(self)
                        .map(|(driver_id, write_half), actor, _ctx| {
                            // Reinsertar el `write_half` en el mapa de `active_drivers`
                            if let Some((_, driver_write_half)) = actor.active_drivers.get_mut(&driver_id) {
                                *driver_write_half = Some(write_half);
                            }
                        }),
                );
            } else {
                debug!("No se pudo enviar el mensaje: no hay conexión activa para el driver {}", driver_id);
            }
        } else {
            debug!("No se encontró un `write_half` para el `driver_id_to_send` especificado: {}", driver_id);
        }

        Ok(())
    }

    /// -------------------------------------------  AUXILIARY FUNCTIONS ------------------------------------------- ///


    /// Returns the id to the closest driver to the passenger that has not been already offered the ride
    /// In case there is no driver available, it returns NO_DRIVER_AVAILABLE
    /// # Arguments
    /// * `message` - The message containing the ride request
    /// # Returns
    /// The id of the closest driver
    pub fn get_closest_driver(&self, message: RideRequest) -> u16 {
        // PickUp position
        let (x_passenger, y_passenger) = (message.x_origin as i32, message.y_origin as i32);

        let mut closest_driver = 0;
        let mut min_distance = i32::MAX;

        for (driver_id, (x_driver, y_driver)) in self.drivers_last_position.iter() {

            // If the driver was already offered the ride, skip it
            if self.ride_manager.driver_has_already_been_offered_ride(message.id, *driver_id) {
                continue;
            }

            let distance = (x_passenger - x_driver).abs() + (y_passenger - y_driver).abs();
            if distance < min_distance {
                min_distance = distance;
                closest_driver = *driver_id;
            }
        }
        closest_driver
    }

    /// Check if the driver is alive, if the driver does not respond in 5 seconds, it is considered dead
    /// If dead, remove the driver from the active drivers and add it to the dead drivers
    /// # Arguments
    /// * `driver_id` - The id of the driver
    /// # Returns
    /// True if the driver is alive, false otherwise
    pub fn update_driver_status(&mut self, driver_id: u16) -> bool {
        // Obtener el tiempo actual
        let time_now = Instant::now();
        let mut is_alive = true;

        // Update the status of the driver
        if let Some(status) = self.drivers_status.get_mut(&driver_id) {
            if let Some(last_response) = status.last_response {
                if time_now.duration_since(last_response).as_secs() > PING_INTERVAL {
                    status.is_alive = false;
                    is_alive = false;
                }
            }
        }
        is_alive
    }


/// ---------------------------------------------------------- LEADER ELECTION IMPLEMENTATION ---------------------------------------------------- ///

    /// Handles the dead leader message as a driver
    /// Si estoy aca es porque me di cuenta que el lider murio
    /// TODO: PROBLEMA: PUEDE QUE OTRO DRIVER TODAVIA NO SE HAYA DADO CUENTA QUE EL LIDER SE CAYO, Y SI ENVIO EL MENSAJE NADIE VA A LEER.

    pub fn handle_dead_leader_as_driver(&mut self, addr: Addr<Driver>) -> Result<(), io::Error> {

        self.state = States::LeaderLess;

        let id = self.id;
        let socket = self.socket.clone();
        let drivers_id = self.drivers_id.clone();

        let stop = Arc::new((Mutex::new(false), Condvar::new()));
        let stop_clone = stop.clone();
        let got_ack = Arc::new((Mutex::new(None), Condvar::new()));
        let leader_id =  Arc::new((Mutex::new(None), Condvar::new()));
        //let drivers_id_arc = Arc::new(RwLock::new(drivers_id));
        let drivers_id_for_leader = Arc::new(RwLock::new(Vec::new()));

        // Lanzar un hilo para el proceso de elección
        let thread_stop = stop.clone();
        let thread_leader_id = leader_id.clone();
        let drivers_id_for_leader_clone = drivers_id_for_leader.clone();

        thread::spawn(move || {
            Self::receive(id, socket, stop_clone, got_ack, leader_id, drivers_id, drivers_id_for_leader_clone);
        });

        // Esperar a que se elija un nuevo líder
        thread::spawn(move || {
            let (lock, cvar) = &*thread_leader_id;
            let mut leader_guard = lock.lock().unwrap();
            while leader_guard.is_none() {
                leader_guard = cvar.wait(leader_guard).unwrap();
            }

            // obtengo los drivers en caso de que sea lider
            let drivers_id: Vec<u16>  = drivers_id_for_leader.read().unwrap().clone();

            if let Some(new_leader) = *leader_guard {
                addr.do_send(NewLeader { leader_id: new_leader, drivers_id });

                // Detener la ejecución del proceso de elección
                let (stop_lock, stop_cvar) = &*thread_stop;
                let mut stop_guard = stop_lock.lock().unwrap();
                *stop_guard = true;
                stop_cvar.notify_all();
            }
        });

        Ok(())
    }

     fn receive(id: u16,
                socket: Arc<UdpSocket>,
                stop: Arc<(Mutex<bool>, Condvar)>,
                got_ack: Arc<(Mutex<Option<u16>>, Condvar)>,
                leader_id_cond: Arc<(Mutex<Option<u16>>, Condvar)>,
                drivers_id: Vec<u16>,
                drivers_id_for_leader: Arc<RwLock<Vec<u16>>>) {

        //tokio::time::sleep(Duration::from_secs(2)); // Provisorio para evitar colisiones iniciales
        let initial_msg = RingMessage::Election { participants: vec![id] };


        // clono y envio mensaje inicial
         let socket_clone = socket.clone();
         let id_clone = id;
         let got_ack_clone = got_ack.clone();
         let msg = serde_json::to_vec(&initial_msg).unwrap();
         let drivers_id_clone = drivers_id.clone();
        thread::spawn(move || {
            Self::safe_send_next(socket_clone, msg, id_clone, got_ack_clone, drivers_id_clone);
        });

        let mut buf = [0u8; 1024];
        loop {
            let (len, addr) = match socket.recv_from(&mut buf) {
                Ok(result) => result,
                Err(e) => {
                    debug!("Error al recibir mensaje: {}", e);
                    break;
                }
            };

            let message: RingMessage = match serde_json::from_slice(&buf[..len]) {
                Ok(msg) => msg,
                Err(e) => {
                    debug!("Error al deserializar el mensaje: {}", e);
                    break;
                }
            };

            match message {
                RingMessage::Ack { id_origin } => {
                    let (lock, cvar) = &*got_ack;
                    {
                        let mut got_ack = lock.lock().unwrap_or_else(|poisoned| {
                            debug!("Lock poisoned, recovering: {:?}", poisoned);
                            poisoned.into_inner()
                        });
                        *got_ack = Some(id_origin);
                    }
                    cvar.notify_all();
                }
                RingMessage::Election { mut participants } => {
                    // Envio ACK
                    let ack_msg = RingMessage::Ack { id_origin: id };
                    let serialized_ack = serde_json::to_vec(&ack_msg).unwrap();
                    if let Err(e) = socket.send_to(&serialized_ack, addr) {
                        debug!("Error al enviar mensaje de ack: {}", e);
                    }

                    if participants.contains(&id) {
                        // Guardo los drivers que participaron de la eleccion
                        {
                            let mut drivers_id_for_leader = drivers_id_for_leader.write().unwrap();
                            *drivers_id_for_leader = participants.clone();
                        }

                        //println!("ENVIANDO MENSAJE DE COORDINADOR");
                        let leader_id = *participants.iter().max().unwrap();
                        let participants_election = vec![id];
                        let msg_to_send = RingMessage::Coordinator {
                            leader_id,
                            participants: participants_election,
                        };
                        let msg = serde_json::to_vec(&msg_to_send).unwrap();
                        // TODO: ver aca, en el codgio de la practica se envia para atras
                        if let Err(e) = socket.send_to(&msg, addr) {
                            debug!("Error al enviar mensaje de ack: {}", e);
                        }

                    } else {
                        //println!("ENVIANDO MENSAJE DE ELECCCION");
                        participants.push(id);
                        let msg_to_send = RingMessage::Election { participants };

                        // clono y envio mensaje
                        let socket_clone = socket.clone();
                        let id_clone = id;
                        let got_ack_clone = got_ack.clone();
                        let msg = serde_json::to_vec(&msg_to_send).unwrap();
                        let drivers_id_clone= drivers_id.clone();
                        thread::spawn(move || {
                            Self::safe_send_next(socket_clone, msg, id_clone, got_ack_clone, drivers_id_clone);
                        });
                    }
                }
                // Mensaje de coordinador: id_origen es el que primero crea el mensaje de coordinador
                RingMessage::Coordinator { leader_id, mut participants } => {
                    *leader_id_cond.0.lock().unwrap() = Some(leader_id);
                    leader_id_cond.1.notify_all();

                    // Envio Ack
                    let ack_msg = RingMessage::Ack { id_origin: id };
                    let serialized_ack = serde_json::to_vec(&ack_msg).unwrap();
                    if let Err(e) = socket.send_to(&serialized_ack, addr) {
                        debug!("Error al enviar mensaje de ack: {}", e);
                    }

                    if !participants.contains(&id) {
                        // sigo el anillo
                        //println!("REENVIANDO MENSAJE DE COORDINADOR");
                        participants.push(id);
                        let msg_to_send = RingMessage::Coordinator { leader_id, participants };
                        let serialized_message = serde_json::to_vec(&msg_to_send).unwrap();
                        let socket_clone = socket.clone();
                        let id_clone = id;
                        let got_ack_clone = got_ack.clone();
                        let drivers_id_clone = drivers_id.clone();
                        thread::spawn(move || {
                            Self::safe_send_next(socket_clone, serialized_message, id_clone, got_ack_clone, drivers_id_clone);
                        });
                    }
                }
            }
        }

    }

    pub fn safe_send_next(
        socket: Arc<UdpSocket>,
        msg: Vec<u8>,
        current_id: u16,
        got_ack: Arc<(Mutex<Option<u16>>, Condvar)>, // Para manejar el Ack
        drivers_id: Vec<u16>,
    ) {

        let mut next_id = current_id + 1; // Lógica para determinar el siguiente nodo

        // si se paso, que vuelva al primero
        if next_id == drivers_id.last().unwrap() + 1 {
            next_id = *drivers_id.first().unwrap();
        }

        // 4000 para que sean 10001, 10002 etc
        let target_address = format!("127.0.0.1:{}", next_id + 4000);

        //println!("Enviando mensaje a {}", next_id);
        // Enviar el mensaje
        if let Err(e) = socket.send_to(&msg, &target_address) {
            debug!("Error enviando mensaje a {}: {}", next_id, e);
            return;
        }

        let got_ack_clone = got_ack.clone();
        let got_ack = got_ack.1.wait_timeout_while(got_ack.0.lock().unwrap(), TIMEOUT, |got_it| got_it.is_none() || got_it.unwrap() != next_id );
        if got_ack.unwrap().1.timed_out() {
            // Si se agotó el tiempo, reintentar con el siguiente
            //println!("[{}] Timeout esperando Ack de {}", current_id, next_id);
            Self::safe_send_next(socket, msg, next_id, got_ack_clone, drivers_id);
        } else {
            //println!("[{}] Ack recibido de {}", current_id, next_id);
        }
    }
}
