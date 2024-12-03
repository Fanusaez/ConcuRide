use crate::models::RideRequest;
use colored::Colorize;
use rand::Rng;
use std::fs::File;
use std::io::{self, BufRead, BufReader};

const BASE_PRICE: u16 = 3000;
const VARIABLE_PRICE: u16 = 50;

/// Returns true if random number generated is lower than probability
/// passed by parameter, or false otherwise
pub fn boolean_with_probability(probability: f64) -> bool {
    let mut rng = rand::thread_rng();
    rng.gen::<f64>() < probability
}

/// Calculates the travel duration based on the distance between the origin and the destination
pub fn calculate_travel_duration(ride_request: &RideRequest) -> u64 {
    let distance = ((ride_request.x_dest as i32 - ride_request.x_origin as i32).abs())
        + ((ride_request.y_dest as i32 - ride_request.y_origin as i32).abs());
    distance as u64
}

/// Calculates price for ride
pub fn calculate_price(ride_request: RideRequest) -> u16 {
    let distance = ((ride_request.x_dest as i32 - ride_request.x_origin as i32).abs()
        + (ride_request.y_dest as i32 - ride_request.y_origin as i32).abs())
        as u16;
    BASE_PRICE + VARIABLE_PRICE * distance
}

/// Reads the configuration file and extracts the drivers ports
pub fn read_config_file(path: &str) -> Result<(Vec<u16>, Vec<u16>), io::Error> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut lines = reader.lines();
    let drivers_ports = if let Some(line) = lines.next() {
        line?
            .split(',')
            .filter_map(|s| s.trim().parse::<u16>().ok())
            .collect()
    } else {
        Vec::new()
    };

    let passengers_ports = if let Some(line) = lines.next() {
        line?
            .split(',')
            .filter_map(|s| s.trim().parse::<u16>().ok())
            .collect()
    } else {
        Vec::new()
    };

    Ok((drivers_ports, passengers_ports))
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
