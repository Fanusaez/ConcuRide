use std::collections::HashMap;
use std::{fs, io};
use serde::Deserialize;


/// Struct que representa las coordenadas de un viaje.
#[derive(Deserialize, Debug)]
pub struct Coordinates {
    pub x_origin: u16,
    pub y_origin: u16,
    pub x_dest: u16,
    pub y_dest: u16,
}

/// Lee un archivo JSON y lo deserializa a un `HashMap<u16, Coordinates>`.
pub fn get_rides(file_path: &str) -> Result<HashMap<u16, Coordinates>, io::Error> {
    let contents = fs::read_to_string(file_path)?;
    let rides: HashMap<u16, Coordinates> = serde_json::from_str(&contents)?;
    Ok(rides)
}