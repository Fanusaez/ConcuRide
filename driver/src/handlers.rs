use std::io;
use actix::{Actor, AsyncContext, Context, Handler, StreamHandler};
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
            self.insert_unpaid_ride(msg).expect("Error adding unpaid ride");
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
    fn handle(&mut self, msg: PaymentAccepted, ctx: &mut Self::Context) -> Self::Result {

        println!("Leader {} received the payment accepted message for passenger with id {}", self.id, msg.id);

        // Remove the ride from the unpaid rides
        let ride_request = {
            let mut unpaid_rides = self.unpaid_rides.write().unwrap();
            unpaid_rides.remove(&msg.id)
        };

        //TODO: hay que ver el tema de los ids de los viajes (No se deberian repetir?)
        match ride_request {
            Some(ride_request) => {
                self.handle_ride_request_as_leader(ride_request).unwrap()
            }
            None => {
                eprintln!("RideRequest with id {} not found in unpaid_rides", msg.id);
            }
        }
    }
}

impl Handler<PaymentRejected> for Driver {
    type Result = ();

    fn handle(&mut self, msg: PaymentRejected, ctx: &mut Self::Context) -> Self::Result {
        // TODO: avisar al cliente que se rechazo el pago de viaje
    }

}


impl Handler<AcceptRide> for Driver {
    type Result = ();
    /// Only received by leader
    /// Ride offered made to driver was accepted
    /// Remove the id of the passenger from ride_and_offers and notify the passenger?
    fn handle(&mut self, msg: AcceptRide, _ctx: &mut Self::Context) -> Self::Result {
        // remove the passenger id from ride_and_offers in case the passenger wants to take another ride
        let mut ride_and_offers = self.ride_and_offers.write().unwrap();
        ride_and_offers.remove(&msg.passenger_id);

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

        self.remove_ride_from_unpaid(msg.passenger_id);
        // TODO: volver a elegir a quien ofrecer el viaje
    }
}

impl Handler<FinishRide> for Driver {
    type Result = ();

    fn handle(&mut self, msg: FinishRide, _ctx: &mut Self::Context) -> Self::Result {
        let is_leader = *self.is_leader.read().unwrap();
        if is_leader {
            println!("Passenger with id {} has been dropped off by driver with id {} ", msg.passenger_id, msg.driver_id);
            self.remove_ride_from_pending(msg.passenger_id);
            // TODO: avisar al pasajero.
        } else {
            // driver send FinishRide to the leader and change state to Idle
            self.finish_ride(msg).unwrap()
        }
    }
}