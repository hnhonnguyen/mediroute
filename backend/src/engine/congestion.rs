use super::router::RoomLoad;

/// Everyone who will be served before a newly arriving patient.
#[allow(dead_code)] // shared head-count helper, exercised by the router tests
pub fn people_ahead(load: &RoomLoad) -> usize {
    let likely: f64 = load
        .likely_arrivals
        .iter()
        .map(|arrival| arrival.weight.max(0.0))
        .sum();
    load.queue_len
        + load.pending_starts
        + load.in_service
        + load.en_route
        + likely.ceil() as usize
}

/// Minutes until a station frees up for the next patient. In-service patients
/// contribute their actual remaining service time; queued, pending, en-route,
/// and likely-arriving patients each consume one service slot in FIFO order.
pub fn est_wait_min(load: &RoomLoad, est_service_min: f64, capacity: usize) -> f64 {
    let base = projected_wait(load, est_service_min, capacity, 0);
    let expected_extra: f64 = load
        .likely_arrivals
        .iter()
        .filter(|arrival| arrival.ready_in_min <= base)
        .map(|arrival| arrival.weight.max(0.0))
        .sum();
    let extra_jobs = expected_extra.ceil() as usize;
    if extra_jobs == 0 {
        base
    } else {
        projected_wait(load, est_service_min, capacity, extra_jobs)
    }
}

pub fn route_cost(
    load: &RoomLoad,
    est_service_min: f64,
    capacity: usize,
    walk_min: f64,
) -> f64 {
    est_wait_min(load, est_service_min, capacity)
        + est_service_min.max(0.0)
        + walk_min.max(0.0)
}

fn projected_wait(
    load: &RoomLoad,
    est_service_min: f64,
    capacity: usize,
    extra_jobs: usize,
) -> f64 {
    let service = est_service_min.max(0.0);
    let capacity = capacity.max(1);
    let downtime = load.downtime_remaining_min.max(0.0);

    // Stations free up when their in-service patient finishes, but a room that
    // is offline cannot hand a station to anyone until it reopens.
    let mut stations: Vec<f64> = load
        .in_service_remaining
        .iter()
        .take(capacity)
        .map(|remaining| remaining.max(0.0).max(downtime))
        .collect();
    while stations.len() < capacity {
        stations.push(downtime);
    }

    let jobs = load.pending_starts + load.queue_len + load.en_route + extra_jobs;
    for _ in 0..jobs {
        let (idx, _) = stations
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| a.total_cmp(b))
            .expect("capacity is at least one");
        stations[idx] += service;
    }

    stations
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min)
        .max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::router::ExpectedArrival;

    fn load(
        queue_len: usize,
        in_service_remaining: &[f64],
        pending_starts: usize,
        en_route: usize,
    ) -> RoomLoad {
        RoomLoad {
            queue_len,
            in_service: in_service_remaining.len(),
            pending_starts,
            en_route,
            in_service_remaining: in_service_remaining.to_vec(),
            ..RoomLoad::default()
        }
    }

    #[test]
    fn idle_room_has_no_wait() {
        assert_eq!(est_wait_min(&RoomLoad::default(), 10.0, 1), 0.0);
        assert_eq!(est_wait_min(&RoomLoad::default(), 10.0, 3), 0.0);
    }

    #[test]
    fn occupied_room_with_empty_queue_waits_only_remaining_service() {
        let occupied = load(0, &[7.5], 0, 0);
        assert!((est_wait_min(&occupied, 10.0, 1) - 7.5).abs() < 1e-9);

        // A second idle station means the patient starts immediately.
        let half_full = load(0, &[7.5], 0, 0);
        assert_eq!(est_wait_min(&half_full, 10.0, 2), 0.0);
    }

    #[test]
    fn wait_grows_with_queue_length() {
        let one_ahead = load(1, &[5.0], 0, 0);
        let two_ahead = load(2, &[5.0], 0, 0);
        let three_ahead = load(3, &[5.0], 0, 0);
        let wait_one = est_wait_min(&one_ahead, 10.0, 1);
        let wait_two = est_wait_min(&two_ahead, 10.0, 1);
        let wait_three = est_wait_min(&three_ahead, 10.0, 1);
        assert!((wait_one - 15.0).abs() < 1e-9);
        assert!((wait_two - 25.0).abs() < 1e-9);
        assert!((wait_three - 35.0).abs() < 1e-9);
    }

    #[test]
    fn parallel_stations_use_the_earliest_free_slot() {
        // Two stations: one frees at 5, one at 12. A queue of two occupies the
        // 5-minute slot twice, so the next patient can start at 12.
        let room = load(2, &[5.0, 12.0], 0, 0);
        assert!((est_wait_min(&room, 10.0, 2) - 15.0).abs() < 1e-9);
    }

    #[test]
    fn pending_starts_and_en_route_patients_are_counted() {
        let room = load(0, &[5.0], 1, 2);
        // 5 -> pending start (15) -> two en-route patients (25, 35).
        assert!((est_wait_min(&room, 10.0, 1) - 35.0).abs() < 1e-9);
    }

    #[test]
    fn downtime_delays_the_earliest_free_station() {
        let mut room = load(0, &[3.0], 0, 0);
        room.downtime_remaining_min = 12.0;
        assert!((est_wait_min(&room, 10.0, 1) - 12.0).abs() < 1e-9);
    }

    #[test]
    fn likely_arrivals_already_racing_here_are_absorbed() {
        let mut room = load(0, &[], 0, 0);
        room.likely_arrivals = vec![ExpectedArrival {
            ready_in_min: 0.0,
            weight: 2.0,
        }];
        let wait = est_wait_min(&room, 10.0, 1);
        assert!((wait - 20.0).abs() < 1e-9, "wait was {wait}");
    }

    #[test]
    fn likely_arrivals_after_patient_can_start_are_ignored() {
        let mut room = load(0, &[], 0, 0);
        room.likely_arrivals = vec![ExpectedArrival {
            ready_in_min: 50.0,
            weight: 1.0,
        }];
        assert_eq!(est_wait_min(&room, 10.0, 1), 0.0);
    }

    #[test]
    fn wait_scales_with_people_and_capacity() {
        let room = RoomLoad {
            queue_len: 3,
            pending_starts: 0,
            in_service: 1,
            en_route: 2,
            in_service_remaining: vec![4.0],
            ..RoomLoad::default()
        };
        assert_eq!(people_ahead(&room), 6);
        // Stations 4. Zero free slots. Jobs: three queued + two en route = 5,
        // leaving the slot at 4 + 5 * 10 = 54.
        assert!((est_wait_min(&room, 10.0, 1) - 54.0).abs() < 1e-9);
        assert!((route_cost(&room, 10.0, 1, 2.0) - 66.0).abs() < 1e-9);
    }

    #[test]
    fn empty_room_cost_is_service_plus_walk() {
        let cost = route_cost(&RoomLoad::default(), 12.0, 3, 2.0);
        assert!((cost - 14.0).abs() < 1e-9);
    }
}
