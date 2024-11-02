use std::collections::HashMap;
use actix::prelude::*;
use std::{fs, io};
use actix::Message;
use serde::{Deserialize, Serialize};


/// Struct que representa las coordenadas de un viaje.
#[derive(Serialize, Deserialize, Debug, Clone, Message)]
#[rtype(result = "()")]
pub struct Coordinates {
    pub x_origin: u16,
    pub y_origin: u16,
    pub x_dest: u16,
    pub y_dest: u16,
}

//impl Message for Coordinates {
   // type Result = ();
//}

/// Lee un archivo JSON y lo deserializa a un `HashMap<u16, Coordinates>`.
pub fn get_rides(file_path: &str) -> Result<HashMap<u16, Coordinates>, io::Error> {
    let contents = fs::read_to_string(file_path)?;
    let rides: HashMap<u16, Coordinates> = serde_json::from_str(&contents)?;
    Ok(rides)
}