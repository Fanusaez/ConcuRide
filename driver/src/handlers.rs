use crate::driver::Driver;
use crate::driver::*;
use crate::models::*;
use crate::utils::*;
use actix::MessageResult;
use actix::{Actor, AsyncContext, Context, Handler, StreamHandler};
use log::*;
use std::io;
use std::sync::atomic::Ordering;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_stream::wrappers::LinesStream;

impl Actor for Driver {
    type Context = Context<Self>;

    /// Called when the actor is started
    /// Starts the ping system if the driver is the leader
    fn started(&mut self, ctx: &mut Self::Context) {
        if self.is_leader.load(Ordering::SeqCst) {
            self.start_ping_system(ctx.address(), ctx);
        } else {
            self.check_leader_alive(ctx.address());
        }
    }

    /// Prevents the actor from stopping if any stream close
    fn stopping(&mut self, _: &mut Self::Context) -> actix::Running {
        // Evita que el actor muera mientras tenga streams activos
        // TODO: puse esta condicion rando, se podria poner otra que sea mas logica
        actix::Running::Continue
    }
}

impl StreamHandler<Result<String, io::Error>> for Driver {
    /// Handles the messages coming from the associated stream.
    ///  Matches the message type and sends it to the corresponding handler.
    fn handle(&mut self, read: Result<String, io::Error>, ctx: &mut Self::Context) {
        if let Ok(line) = read {
            match serde_json::from_str::<MessageType>(&line) {
                Ok(message) => match message {
                    MessageType::RideRequest(coords) => {
                        ctx.address().do_send(coords);
                    }
                    MessageType::AcceptRide(accept_ride) => {
                        ctx.address().do_send(accept_ride);
                    }
                    MessageType::DeclineRide(decline_ride) => {
                        ctx.address().do_send(decline_ride);
                    }
                    MessageType::FinishRide(finish_ride) => {
                        ctx.address().do_send(finish_ride);
                    }
                    MessageType::PaymentAccepted(payment_accepted) => {
                        ctx.address().do_send(payment_accepted);
                    }
                    MessageType::PaymentRejected(payment_rejected) => {
                        ctx.address().do_send(payment_rejected);
                    }
                    MessageType::NewConnection(new_connection) => {
                        ctx.address().do_send(new_connection);
                    }
                    MessageType::PositionUpdate(position_update) => {
                        ctx.address().do_send(position_update);
                    }
                    MessageType::Ping(ping) => {
                        ctx.address().do_send(ping);
                    }
                    MessageType::PayRide(pay_ride) => {
                        ctx.address().do_send(pay_ride);
                    }
                    MessageType::ReStartRideRequest(restart_ride_request) => {
                        ctx.address().do_send(restart_ride_request);
                    }
                    _ => {
                        println!("Unknown Message");
                    }
                },
                Err(e) => {
                    eprintln!("Failed to deserialize message: {}", e);
                }
            }
        } else {
            eprintln!("[{:?}] Failed to read line {:?}", self.id, read);
        }
    }
}

impl Handler<RideRequest> for Driver {
    type Result = ();

    /// Handles the ride request message depending on whether the driver is the leader or not.
    fn handle(&mut self, msg: RideRequest, ctx: &mut Self::Context) -> Self::Result {
        if self.is_leader.load(Ordering::SeqCst) {
            // todo: ojo, si el pasajero se reconcto no deberia imprimirse esto, en handle_ride_request_as_leader se maneja y hay otro TODO
            log(
                &format!("LEADER RECEIVED RIDE REQUEST FROM PASSENGER {}", msg.id),
                "DRIVER",
            );
            self.handle_ride_request_as_leader(msg, ctx)
                .expect("Error handling ride request as leader");
        } else {
            self.handle_ride_request_as_driver(msg, ctx.address(), ctx)
                .expect("Error handling ride request as driver");
        }
    }
}

impl Handler<ReStartRideRequest> for Driver {
    type Result = ();

    /// Handles the restart ride request message
    fn handle(&mut self, msg: ReStartRideRequest, ctx: &mut Self::Context) -> Self::Result {
        if self.is_leader.load(Ordering::SeqCst) {
            debug!("No deberia recibir este mensaje como lider");
        } else {
            self.handle_restart_ride_request_as_driver(msg, ctx)
                .unwrap();
        }
    }
}

impl Handler<PaymentAccepted> for Driver {
    type Result = ();

    /// Only received by leader
    /// Handles the payment accepted message
    fn handle(&mut self, msg: PaymentAccepted, ctx: &mut Self::Context) -> Self::Result {
        if self.is_leader.load(Ordering::SeqCst) {
            self.handle_payment_accepted_as_leader(msg, ctx.address(), ctx)
                .unwrap();
        } else {
            eprintln!(
                "Driver {} is not the leader, should not receive this message",
                self.id
            );
        }
    }
}

impl Handler<PaymentRejected> for Driver {
    type Result = ();
    /// Only received by leader
    fn handle(&mut self, msg: PaymentRejected, ctx: &mut Self::Context) -> Self::Result {
        if self.is_leader.load(Ordering::SeqCst) {
            log(
                &format!("PAYMENT REJECTED FOR PASSENGER {}", msg.id),
                "DRIVER",
            );
            self.handle_payment_rejected_as_leader(msg, ctx).unwrap();
        } else {
            eprintln!(
                "Driver {} is not the leader, should not receive this message",
                self.id
            );
        }
    }
}

impl Handler<AcceptRide> for Driver {
    type Result = ();
    /// Only received by leader
    /// Ride offered made to driver was accepted
    fn handle(&mut self, msg: AcceptRide, _ctx: &mut Self::Context) -> Self::Result {
        if self.is_leader.load(Ordering::SeqCst) {
            self.handle_accept_ride_as_leader(msg).unwrap();
            log(
                &format!(
                    "RIDE REQUEST {} WAS ACCEPTED BY DRIVER {}",
                    msg.passenger_id, msg.driver_id,
                ),
                "DRIVER",
            );
        } else {
            eprintln!(
                "Driver {} is not the leader, should not receive this message",
                self.id
            );
        }
    }
}

impl Handler<DeclineRide> for Driver {
    type Result = ();
    /// Only received by leader
    /// Ride offered made to driver was declined
    fn handle(&mut self, msg: DeclineRide, ctx: &mut Self::Context) -> Self::Result {
        if self.is_leader.load(Ordering::SeqCst) {
            log(
                &format!(
                    "RIDE REQUEST {} WAS DECLINED BY DRIVER {}",
                    msg.passenger_id, msg.driver_id,
                ),
                "DRIVER",
            );
            match self.handle_declined_ride_as_leader(msg, ctx.address(), ctx) {
                Ok(_) => {}
                Err(e) => {
                    eprintln!("Error handling declined ride as leader: {:?}", e);
                }
            }
        } else {
            eprintln!(
                "Driver {} is not the leader, should not receive this message",
                self.id
            );
        }
    }
}

impl Handler<FinishRide> for Driver {
    type Result = ();
    /// Message received by the leader and the driver
    /// If the driver is the leader, will remove the ride from the pending rides and notify the passenger
    /// If the driver is not the leader, will send the message to the leader
    fn handle(&mut self, msg: FinishRide, ctx: &mut Self::Context) -> Self::Result {
        if self.is_leader.load(Ordering::SeqCst) {
            log(
                &format!(
                    "RIDE REQUEST {} WAS FINISHED BY DRIVER {}",
                    msg.passenger_id, msg.driver_id,
                ),
                "DRIVER",
            );
            self.handle_finish_ride_as_leader(msg, ctx).unwrap();
        } else {
            // driver send FinishRide to the leader and change state to Idle
            log(
                &format!(
                    "RIDE REQUEST {} WAS FINISHED BY DRIVER {}",
                    msg.passenger_id, msg.driver_id,
                ),
                "DRIVER",
            );
            self.handle_finish_ride_as_driver(msg, ctx.address(), ctx)
                .unwrap()
        }
    }
}

impl Handler<RestartDriverSearch> for Driver {
    type Result = ();

    /// Handles the restart driver search message
    fn handle(&mut self, msg: RestartDriverSearch, ctx: &mut Self::Context) -> Self::Result {
        if self.is_leader.load(Ordering::SeqCst) {
            self.handle_restart_driver_search_as_leader(msg, ctx.address(), ctx)
                .unwrap();
        } else {
            eprintln!(
                "Driver {} is not the leader, should not receive this message",
                self.id
            );
        }
    }
}

impl Handler<StreamMessage> for Driver {
    type Result = ();
    /// Handles the stream message, this message is received when a new stream is connected to the driver
    /// The driver will add the stream to the list of active streams
    fn handle(&mut self, msg: StreamMessage, _ctx: &mut Self::Context) -> Self::Result {
        if let Some(read_half) = msg.stream {
            Driver::add_stream(LinesStream::new(BufReader::new(read_half).lines()), _ctx);
        } else {
            eprintln!("No se proporcionó un stream válido");
        }
    }
}

impl Handler<WriteHalfLeader> for Driver {
    type Result = ();

    /// Handles the write half leader message, this message is received when the driver is the leader
    /// The driver will store the write half of the stream in the leader_write_half attribute
    fn handle(&mut self, msg: WriteHalfLeader, _ctx: &mut Self::Context) -> Self::Result {
        if self.is_leader.load(Ordering::SeqCst) {
            debug!("No deberia recibir este mensaje como lider")
        } else {
            self.handle_write_half_leader_as_driver(msg).unwrap();
        }
    }
}

/// Handles the new connection message, this message is received as a handshake between the driver and the passenger
/// The driver will store the passenger id and the write half of the stream in the passengers_write_half hashmap
/// If the passenger was already connected, the previous connection will be removed
/// If the passenger was already connected, the driver will check if there is a pending ride request for the passenger
/// If there is a pending ride request, the leader will send
impl Handler<NewConnection> for Driver {
    type Result = ();

    fn handle(&mut self, msg: NewConnection, ctx: &mut Self::Context) -> Self::Result {
        // New passenger id and old passenger id
        let new_passenger_id = msg.passenger_id;
        let old_passenger_id = msg.used_port;

        // Check if the passenger was already connected
        let previous_connection = self.passengers_write_half.contains_key(&new_passenger_id);

        // If previous connection, remove the previous connection
        if previous_connection {
            log(
                &format!("RE-CONNECTED WITH PASSENGER {}", new_passenger_id),
                "INFO",
            );
            match self.passengers_write_half.remove(&new_passenger_id) {
                Some(_write_half) => {
                    debug!("Old connection deleted, passenger id {}", new_passenger_id);
                }
                None => {
                    debug!("Connection not found for passenger {}", new_passenger_id);
                }
            }
        } else {
            log(
                &format!("NEW CONNECTION WITH PASSENGER {}", new_passenger_id),
                "NEW_CONNECTION",
            );
        }

        // Store the new connection
        if let Some(write_half_passenger) = self.passengers_write_half.remove(&old_passenger_id) {
            self.passengers_write_half
                .insert(new_passenger_id, write_half_passenger);
        } else {
            debug!(
                "No previous connection found for passenger {}",
                old_passenger_id
            );
        }

        // If previous connection, verify if there is a pending ride request and inform the passenger
        if previous_connection {
            self.verify_pending_ride_request(new_passenger_id, ctx)
                .unwrap();
        }
    }
}

/// Handles the new passenger connection message, only used by the leader
/// After connecting with passengers as new leader, leader will set up the connection with the new passenger
impl Handler<NewPassengerConnection> for Driver {
    type Result = ();

    fn handle(&mut self, msg: NewPassengerConnection, ctx: &mut Self::Context) -> Self::Result {
        if self.is_leader.load(Ordering::SeqCst) {
            self.handle_new_passenger_connection_as_leader(msg, ctx)
                .unwrap();
        } else {
            eprintln!(
                "Driver {} is not the leader, should not receive this message",
                self.id
            );
        }
    }
}

/// Handles the ping message
/// If the driver is the leader, will handle the ping as leader
/// If the driver is not the leader, will handle the ping as driver
impl Handler<Ping> for Driver {
    type Result = ();

    fn handle(&mut self, msg: Ping, _ctx: &mut Self::Context) -> Self::Result {
        if self.is_leader.load(Ordering::SeqCst) {
            info!("{}", format!("PING RECEIVED FROM DRIVER {}", msg.id_sender));
            self.handle_ping_as_leader(msg).unwrap();
        } else {
            info!("{}", format!("PING RECEIVED FROM LEADER {}", msg.id_sender));
            self.handle_ping_as_driver(msg, _ctx).unwrap();
        }
    }
}

/// Handles the send ping to message
/// If the driver is the leader, will send the ping to the driver
impl Handler<SendPingTo> for Driver {
    type Result = ();

    fn handle(&mut self, msg: SendPingTo, _ctx: &mut Self::Context) -> Self::Result {
        if self.is_leader.load(Ordering::SeqCst) {
            self.send_ping_to_driver(msg.id_to_send, _ctx).unwrap();
        } else {
            eprintln!(
                "Driver {} is not the leader, should not receive this message",
                self.id
            );
        }
    }
}

/// Handles the position update message
impl Handler<PositionUpdate> for Driver {
    type Result = ();

    fn handle(&mut self, msg: PositionUpdate, _ctx: &mut Self::Context) -> Self::Result {
        if self.is_leader.load(Ordering::SeqCst) {
            self.handle_position_update_as_leader(msg).unwrap();
        } else {
            self.handle_position_update_as_driver(msg, _ctx).unwrap();
        }
    }
}

/// Handles the PayRide message, received after finishing a ride
impl Handler<PayRide> for Driver {
    type Result = ();
    fn handle(&mut self, msg: PayRide, _ctx: &mut Self::Context) -> Self::Result {
        if self.is_leader.load(Ordering::SeqCst) {
            eprintln!("Leader should not receive payment message");
        } else {
            log(
                &format!(
                    "DRIVER {} RECEIVED PAYMENT FOR RIDE {}",
                    self.id, msg.ride_id
                ),
                "DRIVER",
            );
        }
    }
}

/// Handles the DeadDriver message, received when a driver is disconnected
impl Handler<DeadDriver> for Driver {
    type Result = ();

    fn handle(&mut self, msg: DeadDriver, ctx: &mut Self::Context) -> Self::Result {
        if self.is_leader.load(Ordering::SeqCst) {
            log(
                &format!("DRIVER {} IS DEAD", msg.driver_id),
                "DISCONNECTION",
            );
            self.handle_dead_driver_as_leader(ctx.address(), msg, ctx)
                .unwrap();
        } else {
            eprintln!(
                "Driver {} is not the leader, should not receive this message",
                self.id
            );
        }
    }
}

/// Handles the DeadLeader message, received when the leader is disconnected
/// Only used by the driver
impl Handler<DeadLeader> for Driver {
    type Result = ();

    fn handle(&mut self, msg: DeadLeader, ctx: &mut Self::Context) -> Self::Result {
        if self.is_leader.load(Ordering::SeqCst) {
            eprintln!("Leader should not receive DeadLeader message");
        } else {
            log("LEADER IS DEAD", "DISCONNECTION");
            self.handle_dead_leader_as_driver(ctx.address()).unwrap();
        }
    }
}

/// Handles the NewLeader message, received when a new leader is appointed
impl Handler<NewLeader> for Driver {
    type Result = ();

    fn handle(&mut self, msg: NewLeader, ctx: &mut Self::Context) -> Self::Result {
        if self.is_leader.load(Ordering::SeqCst) {
            eprintln!("Leader should not receive NewLeader message");
        } else if msg.leader_id == self.id {
            log(
                &format!("I AM THE NEW LEADER {}", msg.leader_id),
                "NEW_CONNECTION",
            );
            self.handle_be_leader_as_driver(msg, ctx.address()).unwrap();
        } else {
            log(
                &format!("NEW LEADER APPOINTED {}", msg.leader_id),
                "NEW_CONNECTION",
            );
            self.handle_new_leader_as_driver(msg, ctx.address())
                .unwrap();
        }
    }
}

/// Handles the NewLeaderAttributes message, received when the new leader sends the new leader attributes
impl Handler<NewLeaderAttributes> for Driver {
    type Result = ();

    fn handle(&mut self, msg: NewLeaderAttributes, ctx: &mut Self::Context) -> Self::Result {
        if self.is_leader.load(Ordering::SeqCst) {
            log("NEW LEADER ATTRIBUTES RECEIVED", "NEW_CONNECTION");
            self.handle_new_leader_attributes_as_leader(msg, ctx.address(), ctx)
                .unwrap();
        } else {
            eprintln!(
                "Driver {} is not the leader, should not receive this message",
                self.id
            );
        }
    }
}

/// Handles the NewPassengerHalfWrite message, used at the start function to store the write half of the passenger
impl Handler<NewPassengerHalfWrite> for Driver {
    type Result = ();

    fn handle(&mut self, msg: NewPassengerHalfWrite, _ctx: &mut Self::Context) -> Self::Result {
        self.passengers_write_half
            .insert(msg.passenger_id, Some(msg.write_half.unwrap()));
    }
}

/// Handles the driver reconnection message, only used by the leader
/// After reconnecting with the driver, the leader will set up the connection with the driver
impl Handler<DriverReconnection> for Driver {
    type Result = ();

    fn handle(&mut self, msg: DriverReconnection, ctx: &mut Self::Context) -> Self::Result {
        if self.is_leader.load(Ordering::SeqCst) {
            self.handle_driver_reconnection_as_leader(msg, ctx);
        } else {
            eprintln!(
                "Driver {} is not the leader, should not receive this message",
                self.id
            );
        }
    }
}

impl Handler<DeadLeaderReconnection> for Driver {
    type Result = ();

    fn handle(&mut self, msg: DeadLeaderReconnection, ctx: &mut Self::Context) -> Self::Result {
        if self.is_leader.load(Ordering::SeqCst) || msg.leader_id != 6000 {
            self.reestablish_connection_with_driver(msg.leader_id, ctx);
        } else {
            eprintln!(
                "Driver {} is not the leader, should not receive this message",
                self.id
            );
        }
    }
}

/// Ping Implementation
impl Actor for LastPingManager {
    type Context = Context<Self>;
}

impl Handler<UpdateLastPing> for LastPingManager {
    type Result = ();

    fn handle(&mut self, msg: UpdateLastPing, _ctx: &mut Self::Context) {
        self.last_ping = msg.time;
    }
}

impl Handler<GetLastPing> for LastPingManager {
    type Result = MessageResult<GetLastPing>;

    fn handle(&mut self, _msg: GetLastPing, _ctx: &mut Self::Context) -> Self::Result {
        MessageResult(self.last_ping)
    }
}
