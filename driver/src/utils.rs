use colored::Colorize;
use rand::Rng;
use crate::models::RideRequest;

const BASE_PRICE: u16 = 3000;
const VARIABLE_PRICE: u16 = 50;

pub fn boolean_with_probability(probability: f64) -> bool {
    let mut rng = rand::thread_rng();
    rng.gen::<f64>() < probability
}

/// Calculates the travel duration based on the distance between the origin and the destination
pub fn calculate_travel_duration(ride_request: &RideRequest) -> u64 {
    let distance = ((ride_request.x_dest as i32 - ride_request.x_origin as i32).abs()) +
        ((ride_request.y_dest as i32 - ride_request.y_origin as i32).abs());
    distance as u64
}

pub fn calculate_price(ride_request: RideRequest) -> u16 {
    let distance = ((ride_request.x_dest as i32 - ride_request.x_origin as i32).abs()
        + (ride_request.y_dest as i32 - ride_request.y_origin as i32).abs()) as u16;
    BASE_PRICE + VARIABLE_PRICE * distance
}

/// TODO: CONSIDERAR PASAR A UN ARCHIVO DE LOGS Y MODULARIZAR
pub fn log(message: &str, type_msg: &str) {
    match type_msg {
        "DRIVER" => println!("[{}] - {}", type_msg, message.blue().bold()),
        "INFO" => println!("[{}] - {}", type_msg, message.cyan().bold()),
        "DISCONNECTION" => println!("[{}] - {}", type_msg, message.red().bold()), // disc = disconnection
        "NEW_CONNECTION" => println!("[{}] - {}", type_msg, message.green().bold()), // nc = no connection
        _ => println!("[{}] - {}", type_msg, message.green().bold()),
    }
}