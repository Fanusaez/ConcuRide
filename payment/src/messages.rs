//! Messages between actors of payment app or between payment app and leader driver

use actix::Message;
use serde::{Deserialize, Serialize};

/// Types of messages that are sent or received
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "message_type")]
pub enum MessageType {
    SendPayment(SendPayment),
    PaymentAccepted(PaymentAccepted),
    PaymentRejected(PaymentRejected),
}

/// Contains the id of the ride and the price for the ride
#[derive(Serialize, Deserialize, Message, Debug, Clone, Copy)]
#[rtype(result = "()")]
pub struct SendPayment {
    pub id: u16,
    pub amount: u16,
}

/// Informs that the payment was accepted
#[derive(Serialize, Deserialize, Message, Debug, Clone, Copy)]
#[rtype(result = "()")]
pub struct PaymentAccepted {
    pub id: u16,
    pub amount: u16,
}

/// Informs that the payment was rejected
#[derive(Serialize, Deserialize, Message, Debug, Clone, Copy)]
#[rtype(result = "()")]
pub struct PaymentRejected {
    pub id: u16,
}

#[derive(Serialize, Deserialize, Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct SendMessage {
    pub msg: String,
}
