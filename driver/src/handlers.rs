use std::fmt::format;
use std::io;
use std::io::Error;
use tokio::io::{split, AsyncBufReadExt, BufReader, AsyncWriteExt, WriteHalf, AsyncReadExt, ReadHalf};
use actix::{Actor, AsyncContext, Context, Handler, StreamHandler};
use tokio_stream::wrappers::LinesStream;

use crate::driver::Driver;
use crate::models::*;
use crate::driver::*;
use crate::utils::*;

impl Actor for Driver {
    type Context = Context<Self>;

    /// Called when the actor is started
    /// Starts the ping system if the driver is the leader
    fn started(&mut self, ctx: &mut Self::Context) {
        if self.is_leader.read().unwrap().clone() {
            self.start_ping_system(ctx.address());
        }
    }

    /// Prevents the actor from stopping if any stream close
    fn stopping(&mut self, _: &mut Self::Context) -> actix::Running {
        // Evita que el actor muera mientras tenga streams activos
        // TODO: puse esta condicion rando, se podria poner otra que sea mas logica
        if !self.active_drivers.read().unwrap().is_empty() {
            actix::Running::Continue
        } else {
            actix::Running::Stop
        }
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

/// TODO: GENERAL DE TODOS LOS HANDLERS, DEBERIAMOS PASAR TODA LA FUNCIONALIDAD A DRIVER, SOLO LOS PRINTS DEJARLOS CAPAZ


impl Handler<RideRequest> for Driver {
    type Result = ();

    /// Handles the ride request message depending on whether the driver is the leader or not.
    fn handle(&mut self, msg: RideRequest, ctx: &mut Self::Context) -> Self::Result {
        let is_leader = *self.is_leader.read().unwrap();

        if is_leader {
            log(&format!("LEADER RECEIVED RIDE REQUEST FROM PASSENGER {}", msg.id), "DRIVER");
            self.handle_ride_request_as_leader(msg).expect("Error handling ride request as leader");
        } else {
            self.handle_ride_request_as_driver(msg, ctx.address()).expect("Error handling ride request as driver");
        }
    }
}


impl Handler<PaymentAccepted> for Driver {
    type Result = ();

    /// Only received by leader
    /// Handles the payment accepted message
    fn handle(&mut self, msg: PaymentAccepted, ctx: &mut Self::Context) -> Self::Result {
        if *self.is_leader.read().unwrap() {
            self.handle_payment_accepted_as_leader(msg, ctx.address()).unwrap();
        }
        else {
            eprintln!("Driver {} is not the leader, should not receive this message", self.id);
        }
    }
}

impl Handler<PaymentRejected> for Driver {
    type Result = ();
    /// Only received by leader
    fn handle(&mut self, msg: PaymentRejected, ctx: &mut Self::Context) -> Self::Result {
        if *self.is_leader.read().unwrap() {
            log(&format!("PAYMENT REJECTED FOR PASSENGER {}", msg.id), "DRIVER");
            self.handle_payment_rejected_as_leader(msg).unwrap();
        }
        else {
            eprintln!("Driver {} is not the leader, should not receive this message", self.id);
        }
    }

}

impl Handler<AcceptRide> for Driver {
    type Result = ();
    /// Only received by leader
    /// Ride offered made to driver was accepted
    fn handle(&mut self, msg: AcceptRide, _ctx: &mut Self::Context) -> Self::Result {
        if *self.is_leader.read().unwrap() {
            self.handle_accept_ride_as_leader(msg).unwrap();
            log(&format!("RIDE REQUEST {} WAS ACCEPTED BY DRIVER {}", msg.passenger_id, msg.driver_id ,), "DRIVER");
        }
        else {
            eprintln!("Driver {} is not the leader, should not receive this message", self.id);
        }
    }
}

impl Handler<DeclineRide> for Driver {
    type Result = ();
    /// Only received by leader
    /// Ride offered made to driver was declined
    fn handle(&mut self, msg: DeclineRide, ctx: &mut Self::Context) -> Self::Result {
        if *self.is_leader.read().unwrap() {
            log(&format!("RIDE REQUEST {} WAS DECLINED BY DRIVER {}", msg.passenger_id, msg.driver_id, ), "DRIVER");
            match self.handle_declined_ride_as_leader(msg, ctx.address()) {
                Ok(_) => {}
                Err(e) => {
                    eprintln!("Error handling declined ride as leader: {:?}", e);
                }
            }
        } else {
            eprintln!("Driver {} is not the leader, should not receive this message", self.id);
        }
    }
}

impl Handler<FinishRide> for Driver {
    type Result = ();
    /// Message received by the leader and the driver
    /// If the driver is the leader, will remove the ride from the pending rides and notify the passenger
    /// If the driver is not the leader, will send the message to the leader
    fn handle(&mut self, msg: FinishRide, _ctx: &mut Self::Context) -> Self::Result {
        let is_leader = *self.is_leader.read().unwrap();
        if is_leader {
            log(&format!("RIDE REQUEST {} WAS FINISHED BY DRIVER {}", msg.passenger_id, msg.driver_id, ), "DRIVER");
            self.handle_finish_ride_as_leader(msg).unwrap();
        } else {
            // driver send FinishRide to the leader and change state to Idle
            log(&format!("RIDE REQUEST {} WAS FINISHED BY DRIVER {}", msg.passenger_id, msg.driver_id, ), "DRIVER");
            self.handle_finish_ride_as_driver(msg).unwrap()
        }
    }
}

impl Handler<RestartDriverSearch> for Driver {
    type Result = ();

    /// Handles the restart driver search message
    fn handle(&mut self, msg: RestartDriverSearch, ctx: &mut Self::Context) -> Self::Result {
        let is_leader = *self.is_leader.read().unwrap();
        if is_leader {
            self.handle_restart_driver_search_as_leader(msg, ctx.address()).unwrap();
        } else {
            eprintln!("Driver {} is not the leader, should not receive this message", self.id);
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

/// Handles the new connection message, this message is received as a handshake between the driver and the passenger
/// The driver will store the passenger id and the write half of the stream in the passengers_write_half hashmap
/// If the passenger was already connected, the previous connection will be removed
/// If the passenger was already connected, the driver will check if there is a pending ride request for the passenger
/// If there is a pending ride request, the leader will send
impl Handler<NewConnection> for Driver {
    type Result = ();

    fn handle(&mut self, msg: NewConnection, _ctx: &mut Self::Context) -> Self::Result {
        let mut passengers_write_half = self.passengers_write_half.write().unwrap();
        let new_passenger_id = msg.passenger_id;
        let old_passenger_id = msg.used_port;

        // Ver si ya estaba la conexion con el pasajero
        let previous_connection = passengers_write_half.contains_key(&new_passenger_id);

        // en caso de reconexion, borrar la conexion anterior
        if previous_connection {
            match passengers_write_half.remove(&new_passenger_id) {
                Some(_write_half) => {
                    eprintln!("Se eliminó la conexión anterior con el pasajero {}", new_passenger_id);
                }
                None => {
                    eprintln!("No se encontró la conexión anterior con el pasajero {}", new_passenger_id);
                }
            }
        }

        // Reemplazar la clave
        if let Some(write_half_passenger) = passengers_write_half.remove(&old_passenger_id) {
            passengers_write_half.insert(new_passenger_id, write_half_passenger);
        } else {
            eprintln!("No se encontró el puerto usado por el pasajero");
        }

        // si ya hubo alguna conexion, verificar que no haya un RideRequest pendiente
        if previous_connection {
            self.verify_pending_ride_request(new_passenger_id).unwrap();
        }

    }
}

impl Handler<Ping> for Driver {
    type Result = ();

    fn handle(&mut self, msg: Ping, _ctx: &mut Self::Context) -> Self::Result {
        let leader = *self.is_leader.read().unwrap();
        if leader {
            self.handle_ping_as_leader(msg).unwrap();
        } else {
            self.handle_ping_as_driver(msg).unwrap();
        }
    }
}


/// Handles the send ping to message
impl Handler<SendPingTo> for Driver {
    type Result = ();

    fn handle(&mut self, msg: SendPingTo, _ctx: &mut Self::Context) -> Self::Result {
        let leader = *self.is_leader.read().unwrap();
        if leader {
            self.send_ping_to_driver(msg.id_to_send).unwrap();
        } else {
            eprintln!("Driver {} is not the leader, should not receive this message", self.id);
        }
    }
}

impl Handler<PositionUpdate> for Driver {
    type Result = ();
    /// Handles the position update message
    /// If the driver is the leader, will update the position of the driver
    fn handle(&mut self, msg: PositionUpdate, _ctx: &mut Self::Context) -> Self::Result {
        if *self.is_leader.read().unwrap() {
            self.handle_position_update_as_leader(msg).unwrap();
        }
        else {
            eprintln!("Driver {} is not the leader, should not receive this message", self.id);
        }
    }
}


impl Handler<PayRide> for Driver {
    type Result = ();
    fn handle(&mut self, msg: PayRide, ctx: &mut Self::Context) -> Self::Result {
        let is_leader = *self.is_leader.read().unwrap();
        if is_leader {
            eprintln!("Leader should not receive payment message");
        } else {
            log(&format!("DRIVER {} RECEIVED PAYMENT FOR RIDE {}", self.id, msg.ride_id), "DRIVER");
        }
    }
}

impl Handler<DeadDriver> for Driver {
    type Result = ();

    fn handle(&mut self, msg: DeadDriver, ctx: &mut Self::Context) -> Self::Result {
        if *self.is_leader.read().unwrap() {
            log(&format!("DRIVER {} IS DEAD", msg.driver_id), "DISC");
            self.handle_dead_driver_as_leader(ctx.address(), msg).unwrap();
        }
        else {
            eprintln!("Driver {} is not the leader, should not receive this message", self.id);
        }
    }
}
