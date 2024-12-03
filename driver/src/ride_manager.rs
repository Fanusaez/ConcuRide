use crate::models::*;
use log::debug;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;
use std::io::ErrorKind;

#[derive(Serialize, Deserialize, Debug)]
pub struct RideManager {
    /// Pending rides, already paid rides, waiting to be accepted by a driver
    pending_rides: HashMap<u16, RideRequest>,
    /// Unpaid rides, waiting for payment confirmation
    unpaid_rides: HashMap<u16, RideRequest>,
    /// Passenger last DriveRequest and the drivers who have been offered the ride
    pub ride_and_offers: HashMap<u16, Vec<u16>>,
    /// Already paid rides (ride_id, PaymentAccepted). It is used to send payment to the driver
    /// from the leader
    paid_rides: HashMap<u16, PaymentAccepted>,

    driver_and_passenger: HashMap<u16, u16>,
}

impl RideManager {
    /// Inserts a ride in the pending rides
    /// # Arguments
    /// * `msg` - The message containing the ride request
    pub fn insert_ride_in_pending(&mut self, msg: RideRequest) -> Result<(), io::Error> {
        // Lo pongo el pending_rides hasta que alguien acepte el viaje
        self.pending_rides.insert(msg.id, msg);
        Ok(())
    }

    pub fn get_pending_ride_requests(&self, passenger_id: u16) -> RideRequest {
        self.pending_rides[&passenger_id]
    }

    /// Removes ride from pending rides
    /// # Arguments
    /// * `passenger_id` - The id of the passenger
    pub fn remove_ride_from_pending(&mut self, passenger_id: u16) -> Result<(), io::Error> {
        if self.pending_rides.remove(&passenger_id).is_none() {
            debug!(
                "RideRequest with id {} not found in pending_rides",
                passenger_id
            );
        }
        Ok(())
    }

    /// Inserts the passenger id and the driver id in the ride_and_offers hashmap
    /// # Arguments
    /// * `passenger_id` - The id of the passenger
    /// * `driver_id` - The id of the driver
    pub fn insert_in_rides_and_offers(
        &mut self,
        passenger_id: u16,
        driver_id: u16,
    ) -> Result<(), io::Error> {
        if let Some(offers) = self.ride_and_offers.get_mut(&passenger_id) {
            offers.push(driver_id);
        } else {
            self.ride_and_offers.insert(passenger_id, vec![driver_id]);
        }
        Ok(())
    }

    /// Removes the passenger id from the ride_and_offers hashmap
    pub fn remove_from_ride_and_offers(&mut self, passenger_id: u16) -> Result<(), io::Error> {
        match self.ride_and_offers.remove(&passenger_id) {
            Some(_) => Ok(()), // Se encontró y eliminó
            None => {
                debug!(
                "RideRequest with id {} not found in ride_and_offers",
                passenger_id
            );
                Ok(())
            }
        }
    }

    /// Removes all the offers from the ride_and_offers hashmap to the passenger_id
    pub fn remove_offers_from_ride_and_offers(
        &mut self,
        passenger_id: u16,
    ) -> Result<(), io::Error> {
        match self.ride_and_offers.get_mut(&passenger_id) {
            Some(offers) => {
                offers.clear();
                Ok(())
            }
            None => {
                println!(
                    "RideRequest with id {} not found in ride_and_offers",
                    passenger_id
                );
                Ok(())
            }
        }
    }

    /// Checks if a driver has already been offered the ride
    pub fn driver_has_already_been_offered_ride(&self, passenger_id: u16, driver_id: u16) -> bool {
        self.ride_and_offers
            .get(&passenger_id)
            .map_or(false, |drivers| drivers.contains(&driver_id))
    }

    /// Upon receiving a ride request, the leader will add it to the unpaid rides
    /// # Arguments
    /// * `msg` - The message containing the ride request
    pub fn insert_unpaid_ride(&mut self, msg: RideRequest) -> Result<(), io::Error> {
        self.unpaid_rides.insert(msg.id, msg);
        Ok(())
    }

    /// Removes unpaid ride from the hashmap
    pub fn remove_unpaid_ride(&mut self, passenger_id: u16) -> Result<RideRequest, io::Error> {
        if let Some(ride_request) = self.unpaid_rides.remove(&passenger_id) {
            Ok(ride_request)
        } else {
            debug!(
                "RideRequest with id {} not found in unpaid_rides",
                passenger_id
            );
            Err(io::Error::new(
                ErrorKind::NotFound,
                "RideRequest not found in unpaid_rides",
            ))
        }
    }

    /// Gets a copy of the Ride Request from the pending rides
    /// # Arguments
    /// * `passenger_id` - The id of the passenger
    /// # Returns
    /// * A copy of the RideRequest
    pub fn get_pending_ride_request(&self, passenger_id: u16) -> Result<RideRequest, io::Error> {
        if let Some(ride_request) = self.pending_rides.get(&passenger_id) {
            Ok(*ride_request)
        } else {
            debug!(
                "RideRequest with id {} not found in pending_rides",
                passenger_id
            );
            Err(io::Error::new(
                ErrorKind::NotFound,
                "RideRequest not found in 'pending_rides'",
            ))
        }
    }

    /// Verifies if a passenger has a pending ride request.
    /// Returns `true` if a pending ride request exists for the given `passenger_id`, otherwise `false`.
    ///
    /// # Arguments
    /// * `passenger_id` - The ID of the passenger.
    ///
    /// # Errors
    /// Returns an `io::Error` if the lock on `pending_rides` cannot be obtained.
    pub fn has_pending_ride_request(&self, passenger_id: u16) -> bool {
        self.pending_rides.contains_key(&passenger_id)
    }

    pub fn new() -> Self {
        RideManager {
            pending_rides: HashMap::new(),
            unpaid_rides: HashMap::new(),
            ride_and_offers: HashMap::new(),
            paid_rides: HashMap::new(),
            driver_and_passenger: HashMap::new(),
        }
    }

    /// Inserts ride in paid_rides with the ride_id as the key and the
    /// PaymentAccepted msg as the value
    pub fn insert_ride_in_paid_rides(
        &mut self,
        ride_id: u16,
        msg: PaymentAccepted,
    ) -> Result<(), io::Error> {
        self.paid_rides.insert(ride_id, msg);
        Ok(())
    }

    /// Removes PaymentAccepted with the information of the payment and returns it
    pub fn get_ride_from_paid_rides(&mut self, ride_id: u16) -> Result<PaymentAccepted, io::Error> {
        match self.paid_rides.remove(&ride_id) {
            Some(msg) => Ok(msg),
            None => Err(io::Error::new(
                ErrorKind::NotFound,
                format!("PaymentAccepted not found for ride_id: {}", ride_id),
            )),
        }
    }

    /// Inserts the driver_id and passenger_id in the driver_and_passenger hashmap
    pub fn insert_driver_and_passenger(
        &mut self,
        driver_id: u16,
        passenger_id: u16,
    ) -> Result<(), io::Error> {
        self.driver_and_passenger.insert(driver_id, passenger_id);
        Ok(())
    }

    /// Removes the driver_id from the driver_and_passenger hashmap
    pub fn remove_driver_and_passenger(&mut self, driver_id: u16) -> Result<(), io::Error> {
        match self.driver_and_passenger.remove(&driver_id) {
            Some(_) => Ok(()),
            None => {
                debug!(
                    "Driver with id {} not found in driver_and_passenger",
                    driver_id
                );
                Ok(())
            }
        }
    }

    pub fn get_driver_and_passenger(&mut self, driver_id: u16) -> Result<(u16, u16), io::Error> {
        match self.driver_and_passenger.get(&driver_id) {
            Some(&passenger_id) => Ok((driver_id, passenger_id)),
            None => Err(io::Error::new(
                ErrorKind::NotFound,
                format!(
                    "Driver with id {} not found in driver_and_passenger",
                    driver_id
                ),
            )),
        }
    }

    pub fn is_driver_assigned_to_passenger(&self, driver_id: u16) -> bool {
        self.driver_and_passenger.contains_key(&driver_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models;
    use models::RideRequest;

    #[test]
    fn test_insert_and_remove_pending_ride() {
        let mut ride_manager = RideManager::new();

        let ride_request = RideRequest {
            id: 1,
            x_origin: 25,
            y_origin: 12,
            x_dest: 16,
            y_dest: 15,
        };

        // Insert ride
        assert!(ride_manager
            .insert_ride_in_pending(ride_request.clone())
            .is_ok());
        {
            assert!(ride_manager.pending_rides.contains_key(&ride_request.id));
            assert_eq!(
                ride_manager
                    .get_pending_ride_request(ride_request.id)
                    .unwrap()
                    .id,
                1
            );
        }

        // Remove ride
        assert!(ride_manager
            .remove_ride_from_pending(ride_request.id)
            .is_ok());
        {
            assert!(!ride_manager.pending_rides.contains_key(&ride_request.id));
            assert_eq!(
                ride_manager.has_pending_ride_request(ride_request.id),
                false
            );
        }
    }

    #[test]
    fn test_insert_and_clear_rides_and_offers() {
        let mut ride_manager = RideManager::new();

        let passenger_id = 1;
        let driver_id = 101;
        let driver_id2 = 102;

        // Insert ride and offer
        assert!(ride_manager
            .insert_in_rides_and_offers(passenger_id, driver_id)
            .is_ok());
        assert!(ride_manager
            .insert_in_rides_and_offers(passenger_id, driver_id2)
            .is_ok());

        assert!(ride_manager.ride_and_offers.contains_key(&passenger_id));
        assert_eq!(
            ride_manager.ride_and_offers[&passenger_id],
            vec![driver_id, driver_id2]
        );

        // Clear offers
        assert!(ride_manager
            .remove_offers_from_ride_and_offers(passenger_id)
            .is_ok());
        assert!(ride_manager.ride_and_offers.contains_key(&passenger_id));
        assert!(ride_manager.ride_and_offers[&passenger_id].is_empty());
    }

    #[test]
    fn test_insert_and_remove_unpaid_ride() {
        let mut ride_manager = RideManager::new();

        let ride_request = RideRequest {
            id: 1,
            x_origin: 25,
            y_origin: 12,
            x_dest: 16,
            y_dest: 15,
        };

        // Insert unpaid ride
        assert!(ride_manager.insert_unpaid_ride(ride_request).is_ok());
        assert!(ride_manager.unpaid_rides.contains_key(&ride_request.id));

        // Remove unpaid ride
        let removed_ride = ride_manager.remove_unpaid_ride(ride_request.id).unwrap();
        assert_eq!(removed_ride.id, ride_request.id);
        assert!(!ride_manager.unpaid_rides.contains_key(&ride_request.id));
    }

    #[test]
    fn test_get_pending_ride_request() {
        let mut ride_manager = RideManager::new();

        let ride_request = RideRequest {
            id: 1,
            x_origin: 25,
            y_origin: 12,
            x_dest: 16,
            y_dest: 15,
        };

        assert!(ride_manager
            .insert_ride_in_pending(ride_request.clone())
            .is_ok());

        let fetched_ride = ride_manager
            .get_pending_ride_request(ride_request.id)
            .unwrap();
        assert_eq!(fetched_ride.id, ride_request.id);
    }
}
