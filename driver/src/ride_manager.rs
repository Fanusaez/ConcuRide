use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::sync::RwLock;

use crate::models::*;

pub struct RideManager {
    /// Pending rides, already paid rides, waiting to be accepted by a driver
    pub pending_rides: Arc<RwLock<HashMap<u16, RideRequest>>>,
    /// Unpaid rides, waiting for payment confirmation
    pub unpaid_rides: Arc<RwLock<HashMap<u16, RideRequest>>>,
    /// Passenger last DriveRequest and the drivers who have been offered the ride
    pub ride_and_offers: Arc<RwLock<HashMap<u16, Vec<u16>>>>,
    /// Already paid rides (ride_id, PaymentAccepted). It is used to send payment to the driver
    /// from the leader
    pub paid_rides: Arc<RwLock<HashMap<u16, PaymentAccepted>>>,
}

impl RideManager {
    /// Inserts a ride in the pending rides
    /// # Arguments
    /// * `msg` - The message containing the ride request
    pub fn insert_ride_in_pending(&self, msg: RideRequest) -> Result<(), io::Error> {
        // Lo pongo el pending_rides hasta que alguien acepte el viaje
        let mut pending_rides = self.pending_rides.write();
        match pending_rides {
            Ok(mut pending_rides) => {
                pending_rides.insert(msg.id, msg);
            }
            Err(e) => {
                eprintln!("Error obtaining writing lock from `pending_rides`: {:?}", e);
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "Error obtaining writing lock from `pending_rides`",
                ));
            }
        }
        Ok(())
    }

    /// Removes ride from pending rides
    /// # Arguments
    /// * `passenger_id` - The id of the passenger
    pub fn remove_ride_from_pending(&self, passenger_id: u16) -> Result<(), io::Error> {
        let mut pending_rides = self.pending_rides.write();
        match pending_rides {
            Ok(mut pending_rides) => {
                if pending_rides.remove(&passenger_id).is_none() {
                    eprintln!(
                        "RideRequest with id {} not found in pending_rides",
                        passenger_id
                    );
                }
            }
            Err(e) => {
                eprintln!("Error obtaining writing lock from `pending_rides`: {:?}", e);
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "Error obtaining writing lock from `pending_rides`",
                ));
            }
        }
        Ok(())
    }

    /// Inserts the passenger id and the driver id in the ride_and_offers hashmap
    /// # Arguments
    /// * `passenger_id` - The id of the passenger
    /// * `driver_id` - The id of the driver
    pub fn insert_in_rides_and_offers(
        &self,
        passenger_id: u16,
        driver_id: u16,
    ) -> Result<(), io::Error> {
        let mut ride_and_offers = self.ride_and_offers.write();
        match ride_and_offers {
            Ok(mut ride_and_offers) => {
                if let Some(offers) = ride_and_offers.get_mut(&passenger_id) {
                    offers.push(driver_id);
                } else {
                    ride_and_offers.insert(passenger_id, vec![driver_id]);
                }
            }
            Err(e) => {
                eprintln!(
                    "Error obtaining writing lock from `ride_and_offers`: {:?}",
                    e
                );
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "Error obtaining writing lock from `ride_and_offers`",
                ));
            }
        }
        Ok(())
    }

    /// Removes the passenger id from the ride_and_offers hashmap
    pub fn remove_from_ride_and_offers(&self, passenger_id: u16) -> Result<(), io::Error> {
        let mut ride_and_offers = self.ride_and_offers.write();
        match ride_and_offers {
            Ok(mut ride_and_offers) => {
                if ride_and_offers.remove(&passenger_id).is_none() {
                    eprintln!(
                        "RideRequest with id {} not found in ride_and_offers",
                        passenger_id
                    );
                }
            }
            Err(e) => {
                eprintln!(
                    "Error obtaining writing lock from `ride_and_offers`: {:?}",
                    e
                );
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "Error obtaining writing lock from `ride_and_offers`",
                ));
            }
        }
        Ok(())
    }

    /// Removes all the offers from the ride_and_offers hashmap to the passenger_id
    pub fn remove_offers_from_ride_and_offers(&self, passenger_id: u16) -> Result<(), io::Error> {
        let mut ride_and_offers = self.ride_and_offers.write();
        match ride_and_offers {
            Ok(mut ride_and_offers) => {
                if let Some(offers) = ride_and_offers.get_mut(&passenger_id) {
                    offers.clear();
                }
            },
            Err(e) => {
                eprintln!("Error obtaining writing lock from `ride_and_offers`: {:?}", e);
                return Err(io::Error::new(io::ErrorKind::Other, "Error obtaining writing lock from `ride_and_offers`"));
            }
        }
        Ok(())
    }

    /// Upon receiving a ride request, the leader will add it to the unpaid rides
    /// # Arguments
    /// * `msg` - The message containing the ride request
    pub fn insert_unpaid_ride(&self, msg: RideRequest) -> Result<(), io::Error> {
        let mut unpaid_rides = self.unpaid_rides.write();
        match unpaid_rides {
            Ok(mut unpaid_rides) => {
                unpaid_rides.insert(msg.id, msg);
            }
            Err(e) => {
                eprintln!("Error obtaining writing lock from `unpaid_rides`: {:?}", e);
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "Error obtaining writing lock from `unpaid_rides`",
                ));
            }
        }
        Ok(())
    }

    /// Removes unpaid ride from the hashmap
    pub fn remove_unpaid_ride(&self, passenger_id: u16) -> Result<RideRequest, io::Error> {
        let mut unpaid_rides = self.unpaid_rides.write().map_err(|e| {
            eprintln!("Error obtaining writing lock from `unpaid_rides`: {:?}", e);
            io::Error::new(
                io::ErrorKind::Other,
                "Error obtaining writing lock from `unpaid_rides`",
            )
        })?;

        if let Some(ride_request) = unpaid_rides.remove(&passenger_id) {
            Ok(ride_request)
        } else {
            eprintln!(
                "RideRequest with id {} not found in unpaid_rides",
                passenger_id
            );
            Err(io::Error::new(
                io::ErrorKind::NotFound,
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
        let pending_rides = self.pending_rides.read().map_err(|e| {
            eprintln!("Error obtaining writing lock from `pending_rides`: {:?}", e);
            io::Error::new(
                io::ErrorKind::Other,
                "Error obtaining writing lock from `pending_rides`",
            )
        })?;

        if let Some(ride_request) = pending_rides.get(&passenger_id) {
            Ok(ride_request.clone())
        } else {
            eprintln!(
                "RideRequest with id {} not found in pending_rides",
                passenger_id
            );
            Err(io::Error::new(
                io::ErrorKind::NotFound,
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
    pub fn has_pending_ride_request(&self, passenger_id: u16) -> Result<bool, io::Error> {
        self.pending_rides
            .read()
            .map(|pending_rides| pending_rides.contains_key(&passenger_id))
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::Other,
                    "Failed to acquire read lock on `pending_rides`",
                )
            })
    }

    pub fn new() -> Self {
        RideManager {
            pending_rides: Arc::new(RwLock::new(HashMap::new())),
            unpaid_rides: Arc::new(RwLock::new(HashMap::new())),
            ride_and_offers: Arc::new(RwLock::new(HashMap::new())),
            paid_rides: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Inserts ride in paid_rides with the ride_id as the key and the
    /// PaymentAccepted msg as the value
    pub fn insert_ride_in_paid_rides(
        &self,
        ride_id: u16,
        msg: PaymentAccepted,
    ) -> Result<(), io::Error> {
        let mut rides = self.paid_rides.write().map_err(|e| {
            eprintln!("Error obtaining write lock: {:?}", e);
            io::Error::new(io::ErrorKind::Other, "PoisonError")
        })?;
        rides.insert(ride_id, msg);
        Ok(())
    }

    /// Removes PaymentAccepted with the information of the payment and returns it
    pub fn get_ride_from_paid_rides(&self, ride_id: u16) -> Result<PaymentAccepted, io::Error> {
        let mut paid_rides = self.paid_rides.write().map_err(|e| {
            eprintln!(
                "Error obtaining writing lock from `paid_rides`: {:?}",
                e
            );
            io::Error::new(
                io::ErrorKind::Other,
                "Error writing lock from `paid_rides`",
            )
        })?;
        if let Some(payment) = paid_rides.remove(&ride_id) {
            Ok(payment)
        } else {
            eprintln!(
                "PaymentAccepted with id {} not found in paid_rides",
                ride_id
            );
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "PaymentAccepted not found in paid_rides",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use models::RideRequest;
    use crate::models;


    #[test]
    fn test_insert_and_remove_pending_ride() {
        let ride_manager = RideManager::new();

        let ride_request = RideRequest {
            id: 1,
            x_origin: 25,
            y_origin: 12,
            x_dest: 16,
            y_dest: 15,
        };

        // Insert ride
        assert!(ride_manager.insert_ride_in_pending(ride_request.clone()).is_ok());
        {
            let pending_rides = ride_manager.pending_rides.read().unwrap();
            assert!(pending_rides.contains_key(&ride_request.id));
            assert_eq!(pending_rides.get(&ride_request.id).unwrap().id, 1);
        }

        // Remove ride
        assert!(ride_manager.remove_ride_from_pending(ride_request.id).is_ok());
        {
            let pending_rides = ride_manager.pending_rides.read().unwrap();
            assert!(!pending_rides.contains_key(&ride_request.id));
        }
    }

    #[test]
    fn test_insert_and_clear_rides_and_offers() {
        let ride_manager = RideManager::new();

        let passenger_id = 1;
        let driver_id = 101;
        let driver_id2 = 102;

        // Insert ride and offer
        assert!(ride_manager.insert_in_rides_and_offers(passenger_id, driver_id).is_ok());
        assert!(ride_manager.insert_in_rides_and_offers(passenger_id, driver_id2).is_ok());
        {
            let ride_and_offers = ride_manager.ride_and_offers.read().unwrap();
            assert!(ride_and_offers.contains_key(&passenger_id));
            assert_eq!(ride_and_offers[&passenger_id], vec![driver_id, driver_id2]);
        }

        // Clear offers
        assert!(ride_manager.remove_offers_from_ride_and_offers(passenger_id).is_ok());
        {
            let ride_and_offers = ride_manager.ride_and_offers.read().unwrap();
            assert!(ride_and_offers.contains_key(&passenger_id));
            assert!(ride_and_offers[&passenger_id].is_empty());
        }
    }

    #[test]
    fn test_insert_and_remove_unpaid_ride() {
        let ride_manager = RideManager::new();

        let ride_request = RideRequest {
            id: 1,
            x_origin: 25,
            y_origin: 12,
            x_dest: 16,
            y_dest: 15,
        };

        // Insert unpaid ride
        assert!(ride_manager.insert_unpaid_ride(ride_request.clone()).is_ok());
        {
            let unpaid_rides = ride_manager.unpaid_rides.read().unwrap();
            assert!(unpaid_rides.contains_key(&ride_request.id));
        }

        // Remove unpaid ride
        let removed_ride = ride_manager.remove_unpaid_ride(ride_request.id).unwrap();
        assert_eq!(removed_ride.id, ride_request.id);
        {
            let unpaid_rides = ride_manager.unpaid_rides.read().unwrap();
            assert!(!unpaid_rides.contains_key(&ride_request.id));
        }
    }

    #[test]
    fn test_get_pending_ride_request() {
        let ride_manager = RideManager::new();

        let ride_request = RideRequest {
            id: 1,
            x_origin: 25,
            y_origin: 12,
            x_dest: 16,
            y_dest: 15,
        };

        assert!(ride_manager.insert_ride_in_pending(ride_request.clone()).is_ok());

        let fetched_ride = ride_manager.get_pending_ride_request(ride_request.id).unwrap();
        assert_eq!(fetched_ride.id, ride_request.id);
    }
}
