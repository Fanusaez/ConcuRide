use actix::Message;
use serde::{Deserialize, Serialize};
use tokio::io::{ReadHalf};
use tokio::net::{TcpListener, TcpStream};

/// RideRequest struct, ver como se puede importar desde otro archivo, esto esta en utils.rs\
#[derive(Serialize, Deserialize, Message, Debug, Clone, Copy)]
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
pub struct AcceptRide {
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
pub struct FinishRide {
    pub passenger_id: u16,
    pub driver_id: u16,
}

#[derive(Serialize, Deserialize, Message, Debug, Clone, Copy)]
#[rtype(result = "()")]
pub struct SendPayment {
    pub id: u16,
    pub amount: i32,
}

#[derive(Serialize, Deserialize, Message, Debug, Clone, Copy)]
#[rtype(result = "()")]
pub struct PaymentRejected {
    pub id: u16,
}

#[derive(Serialize, Deserialize, Message, Debug, Clone, Copy)]
#[rtype(result = "()")]
pub struct PaymentAccepted {
    pub id: u16,
}

#[derive(Message, Debug)]
#[rtype(result = "()")]
pub struct StreamMessage {
    pub stream: Option<ReadHalf<TcpStream>>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Message)]
#[rtype(result = "()")]
pub struct NewConnection {
    pub passenger_id: u16,
    pub used_port: u16,
}


#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "message_type")]
/// enum Message used to deserialize
pub enum MessageType {
    RideRequest(RideRequest),
    AcceptRide(AcceptRide),
    DeclineRide(DeclineRide),
    FinishRide(FinishRide),
    SendPayment(SendPayment),
    PaymentAccepted(PaymentAccepted),
    PaymentRejected(PaymentRejected),
    NewConnection(NewConnection),
}