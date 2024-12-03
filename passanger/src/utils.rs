use crate::models::*;
use colored::Colorize;
use std::collections::HashMap;
use std::{fs, io};

/// Reads JSON file and deserializes it into a `HashMap<u16, Coordinates>`.
pub fn get_rides(file_path: &str) -> Result<HashMap<u16, RideRequest>, io::Error> {
    let contents = fs::read_to_string(file_path)?;
    let rides: HashMap<u16, RideRequest> = serde_json::from_str(&contents)?;
    Ok(rides)
}

pub fn log(message: &str, type_msg: &str) {
    match type_msg {
        "DRIVER" => println!("[{}] - {}", type_msg, message.blue().bold()),
        "INFO" => println!("[{}] - {}", type_msg, message.cyan().bold()),
        "DISCONNECTION" => println!("[{}] - {}", type_msg, message.red().bold()), // disc = disconnection
        "NEW_CONNECTION" => println!("[{}] - {}", type_msg, message.green().bold()), // nc = no connection
        _ => println!("[{}] - {}", type_msg, message.green().bold()),
    }
}
