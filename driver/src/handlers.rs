use std::io;
use tokio::io::{split, AsyncBufReadExt, BufReader, AsyncWriteExt, WriteHalf, AsyncReadExt, ReadHalf};
use actix::{Actor, AsyncContext, Context, Handler, StreamHandler};
use tokio_stream::wrappers::LinesStream;

use crate::driver::Driver;
use crate::models::*;
use crate::driver::*;

impl Actor for Driver {
    type Context = Context<Self>;
}

impl StreamHandler<Result<String, io::Error>> for Driver {
    /// Handles the messages coming from the associated stream.
    /// Matches the message type and sends it to the corresponding handler.
    fn handle(&mut self, read: Result<String, io::Error>, ctx: &mut Self::Context) {
        if let Ok(line) = read {
            let message: MessageType = serde_json::from_str(&line).expect("Failed to deserialize message");
            match message {
                MessageType::RideRequest(coords)=> {
                    ctx.address().do_send(coords);
                }

                MessageType::AcceptRide (accept_ride) => {
                    ctx.address().do_send(accept_ride);
                }

                MessageType::DeclineRide (decline_ride) => {
                    ctx.address().do_send(decline_ride);
                }

                MessageType::FinishRide (finish_ride) => {
                    ctx.address().do_send(finish_ride);
                }

                MessageType::PaymentAccepted(payment_accepted) => {
                    println!("Payment accepted msg received");
                    ctx.address().do_send(payment_accepted);
                }

                MessageType::PaymentRejected(payment_rejected) => {
                    ctx.address().do_send(payment_rejected);
                }
                _ => {
                    println!("Unknown Message");
                }
            }
        } else {
            println!("[{:?}] Failed to read line {:?}", self.id, read);
        }
    }
}

impl Handler<RideRequest> for Driver {
    type Result = ();

    /// Handles the ride request message depending on whether the driver is the leader or not.
    fn handle(&mut self, msg: RideRequest, ctx: &mut Self::Context) -> Self::Result {
        let is_leader = *self.is_leader.read().unwrap();

        if is_leader {
            println!("Leader recived ride request from passenger {}", msg.id);
            self.ride_manager.insert_unpaid_ride(msg).expect("Error adding unpaid ride");
            self.send_payment(msg).expect("Error sending payment");
        } else {
            self.handle_ride_request_as_driver(msg, ctx.address()).expect("Error handling ride request as driver");
        }
    }
}


impl Handler<PaymentAccepted> for Driver {
    type Result = ();

    /// Only receved by leader
    /// Handles the payment accepted message
    fn handle(&mut self, msg: PaymentAccepted, _ctx: &mut Self::Context) -> Self::Result {

        println!("Leader {} received the payment accepted message for passenger with id {}", self.id, msg.id);

        let ride_request = self.ride_manager.remove_unpaid_ride(msg.id);

        match ride_request {
            Ok(ride_request) => {
                self.handle_ride_request_as_leader(ride_request).unwrap()
            }
            Err(e) => {
                eprintln!("Error removing unpaid ride: {:?}", e);
            }
        }
    }
}

impl Handler<PaymentRejected> for Driver {
    type Result = ();

    fn handle(&mut self, msg: PaymentRejected, ctx: &mut Self::Context) -> Self::Result {
        let msg_message_type = MessageType::PaymentRejected(msg);
        match self.send_message(msg_message_type) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("Error sending message to passenger: {:?}", e);
            }
        }
    }

}


impl Handler<AcceptRide> for Driver {
    type Result = ();
    /// Only received by leader
    /// Ride offered made to driver was accepted
    /// Remove the id of the passenger from ride_and_offers and notify the passenger?
    fn handle(&mut self, msg: AcceptRide, _ctx: &mut Self::Context) -> Self::Result {
        // remove the passenger id from ride_and_offers in case the passenger wants to take another ride
        match self.ride_manager.remove_from_ride_and_offers(msg.passenger_id) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("Error removing passenger id from ride_and_offers: {:?}", e);
            }
        }

        /// TODO: Pending_rides se saca una vez que notifico al pasajero
        println!("Ride with id {} was accepted by driver {}", msg.passenger_id, msg.driver_id);
    }
}

impl Handler<DeclineRide> for Driver {
    type Result = ();
    /// Only received by leader
    /// Ride offered made to driver was declined
    /// TODO: OFFER THE RIDE TO ANOTHER DRIVER
    fn handle(&mut self, msg: DeclineRide, _ctx: &mut Self::Context) -> Self::Result {
        println!("Lider {} received the declined message for the ride request from driver {}", self.id, msg.driver_id);
        match self.handle_declined_ride_as_leader(msg) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("Error handling declined ride as leader: {:?}", e);
            }
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
            println!("Passenger with id {} has been dropped off by driver with id {} ", msg.passenger_id, msg.driver_id);
            match self.ride_manager.remove_ride_from_pending(msg.passenger_id) {
                Ok(_) => {}
                Err(e) => {
                    eprintln!("Error removing ride from pending rides: {:?}", e);
                }
            }
            let msg_message_type = MessageType::FinishRide(msg);
            match self.send_message(msg_message_type) {
                Ok(_) => {}
                Err(e) => {
                    eprintln!("Error sending message to passenger: {:?}", e);
                }
            }
        } else {
            // driver send FinishRide to the leader and change state to Idle
            self.finish_ride(msg).unwrap()
        }
    }
}

impl Handler<StreamMessage> for Driver {
    type Result = ();

    fn handle(&mut self, msg: StreamMessage, _ctx: &mut Self::Context) -> Self::Result {
        self.add_stream(LinesStream::new(BufReader::new(msg.stream).lines()))
    }
}