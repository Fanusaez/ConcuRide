use std::collections::HashMap;
use actix::prelude::*;
use std::{fs, io};

use crate::models::*;


/// Lee un archivo JSON y lo deserializa a un `HashMap<u16, Coordinates>`.
pub fn get_rides(file_path: &str) -> Result<HashMap<u16, RideRequest>, io::Error> {
    let contents = fs::read_to_string(file_path)?;
    let rides: HashMap<u16, RideRequest> = serde_json::from_str(&contents)?;
    Ok(rides)
}