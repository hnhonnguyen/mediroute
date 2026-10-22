use crate::domain::WALK_TIME_MIN;

use super::router::RoomLoad;

/// Everyone who will be served before a newly arriving patient.
pub fn people_ahead(load: &RoomLoad) -> usize {
    load.queue_len + load.pending_starts + load.in_service + load.en_route
}

pub fn est_wait_min(load: &RoomLoad, est_service_min: f64, capacity: usize) -> f64 {
    people_ahead(load) as f64 * est_service_min.max(0.0) / capacity.max(1) as f64
}

pub fn route_cost(load: &RoomLoad, est_service_min: f64, capacity: usize) -> f64 {
    est_wait_min(load, est_service_min, capacity) + est_service_min.max(0.0) + WALK_TIME_MIN
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_scales_with_people_and_capacity() {
        let load = RoomLoad {
            queue_len: 3,
            pending_starts: 0,
            in_service: 1,
            en_route: 2,
        };
        assert_eq!(people_ahead(&load), 6);
        assert!((est_wait_min(&load, 10.0, 2) - 30.0).abs() < 1e-9);
        assert!((route_cost(&load, 10.0, 2) - 42.0).abs() < 1e-9);
    }

    #[test]
    fn empty_room_cost_is_service_plus_walk() {
        let cost = route_cost(&RoomLoad::default(), 12.0, 3);
        assert!((cost - 14.0).abs() < 1e-9);
    }
}
