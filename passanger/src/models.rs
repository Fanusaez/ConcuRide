//! Contains messages used by Passenger or TcpSender

use actix::Message;
use serde::{Deserialize, Serialize};
use tokio::io::{ReadHalf, WriteHalf};
use tokio::net::TcpStream;

/// Contains the ride_id, the origin coordinates and destination coordinates
#[derive(Serialize, Deserialize, Debug, Clone, Message)]
#[rtype(result = "()")]
pub struct RideRequest {
    pub id: u16,
    pub x_origin: u16,
    pub y_origin: u16,
    pub x_dest: u16,
    pub y_dest: u16,
}

/// Contains the passanger id and the driver id who made the ride
#[derive(Serialize, Deserialize, Message, Debug, Clone, Copy)]
#[rtype(result = "()")]
pub struct FinishRide {
    pub passenger_id: u16,
    pub driver_id: u16,
}

/// Contains the passanger id and the id of the driver who declined
/// the ride
#[derive(Serialize, Deserialize, Message, Debug, Clone, Copy)]
#[rtype(result = "()")]
pub struct DeclineRide {
    pub passenger_id: u16,
    pub driver_id: u16,
}

/// Contains the id of the ride whose payment was rejected
#[derive(Serialize, Deserialize, Message, Debug, Clone, Copy)]
#[rtype(result = "()")]
pub struct PaymentRejected {
    pub id: u16,
}

/// Contains driver id, passanger id and current position of the driver
#[derive(Serialize, Deserialize, Debug, Clone, Message)]
#[rtype(result = "()")]
pub struct PositionUpdate {
    pub driver_id: u16,
    pub passenger_id: u16,
    pub current_position: (u32, u32),
}

/// Contains the passanger id and the used port
#[derive(Serialize, Deserialize, Debug, Clone, Message)]
#[rtype(result = "()")]
pub struct NewConnection {
    pub passenger_id: u16,
    pub used_port: u16,
}

/// Contains the passenger id and the state as a string
#[derive(Serialize, Deserialize, Debug, Clone, Message)]
#[rtype(result = "()")]
pub struct RideRequestReconnection {
    pub passenger_id: u16,
    pub state: String,
}

/// Contains an Option of the read half part of a Tcp stream and an
/// Option of the write half of the same stream
#[derive(Message, Debug)]
#[rtype(result = "()")]
pub struct NewLeaderStreams {
    pub read: Option<ReadHalf<TcpStream>>,
    pub write_half: Option<WriteHalf<TcpStream>>,
}

/// Used to stop the actors context to free resources
#[derive(Message, Debug)]
#[rtype(result = "()")]
pub struct StopActor;

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "message_type")]
/// enum Message used to deserialize
pub enum MessageType {
    RideRequest(RideRequest),
    FinishRide(FinishRide),
    DeclineRide(DeclineRide),
    PaymentRejected(PaymentRejected),
    PositionUpdate(PositionUpdate),
    NewConnection(NewConnection),
    RideRequestReconnection(RideRequestReconnection),
}
