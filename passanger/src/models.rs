use actix::Message;
use serde::{Deserialize, Serialize};
use tokio::io::{ReadHalf, WriteHalf};
use tokio::net::TcpStream;

// Struct que representa las coordenadas de un viaje.
#[derive(Serialize, Deserialize, Debug, Clone, Message)]
#[rtype(result = "()")]
pub struct RideRequest {
    pub id: u16,
    pub x_origin: u16,
    pub y_origin: u16,
    pub x_dest: u16,
    pub y_dest: u16,
}

#[derive(Serialize, Deserialize, Message, Debug, Clone, Copy)]
#[rtype(result = "()")]
pub struct FinishRide {
    pub passenger_id: u16,
    pub driver_id: u16,
}

#[derive(Serialize, Deserialize, Message, Debug, Clone, Copy)]
#[rtype(result = "()")]
pub struct DeclineRide {
    pub passenger_id: u16,
    pub driver_id: u16,
}

#[derive(Serialize, Deserialize, Message, Debug, Clone, Copy)]
#[rtype(result = "()")]
pub struct PaymentRejected {
    pub id: u16,
}

// se usaria para que el pasajero sepa la posicion, despues ver si se usa o no
#[derive(Serialize, Deserialize, Debug, Clone, Message)]
#[rtype(result = "()")]
pub struct PositionUpdate {
    pub driver_id: u16,
    pub passenger_id: u16,
    pub current_position: (u32, u32),
}

#[derive(Serialize, Deserialize, Debug, Clone, Message)]
#[rtype(result = "()")]
pub struct NewConnection {
    pub passenger_id: u16,
    pub used_port: u16,
}

#[derive(Serialize, Deserialize, Debug, Clone, Message)]
#[rtype(result = "()")]
pub struct RideRequestReconnection {
    pub passenger_id: u16,
    pub state: String,
}

#[derive(Message, Debug)]
#[rtype(result = "()")]
pub struct NewLeaderStreams {
    pub read: Option<ReadHalf<TcpStream>>,
    pub write_half: Option<WriteHalf<TcpStream>>,
}

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