use super::router::RoomLoad;
use crate::scenario::ArrivalDemand;

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

/// Projected wait for a patient who reaches `room` `arrive_in_min` from now.
/// The station backlog is identical to `est_wait_min`, but a station that is
/// already free when the patient arrives costs no wait. In particular, an
/// outage that ends before the patient reaches the room is not charged, while
/// a long outage still floors the earliest usable station at its reopening.
pub fn est_wait_at_arrival_min(
    load: &RoomLoad,
    est_service_min: f64,
    capacity: usize,
    arrive_in_min: f64,
) -> f64 {
    let arrive = arrive_in_min.max(0.0);
    let base = projected_wait_at(load, est_service_min, capacity, 0, arrive);
    // Once the patient's projected start is known, competitors that become
    // free before that moment can still slip ahead in the queue.
    let start_from_now = arrive + base;
    let expected_extra: f64 = load
        .likely_arrivals
        .iter()
        .filter(|arrival| arrival.ready_in_min <= start_from_now)
        .map(|arrival| arrival.weight.max(0.0))
        .sum();
    let extra_jobs = expected_extra.ceil() as usize;
    if extra_jobs == 0 {
        base
    } else {
        projected_wait_at(load, est_service_min, capacity, extra_jobs, arrive)
    }
}

/// Projected wait for a patient who arrives `horizon_min` from now. The
/// current cohort keeps the station model of `est_wait_min`, while expected
/// arrivals from the scenario demand model consume a station at their arrival
/// minute. During a rush, arrivals can outpace the drain rate and the wait
/// grows; during the lull or after the last burst no arrivals are added and
/// the remaining load drains at capacity until it reaches zero.
pub fn future_wait_min(
    load: &RoomLoad,
    est_service_min: f64,
    capacity: usize,
    demand: Option<&ArrivalDemand>,
    room: &str,
    now_min: f64,
    horizon_min: f64,
) -> f64 {
    let horizon = horizon_min.max(0.0);
    let service = est_service_min.max(0.0);
    let capacity = capacity.max(1);
    let downtime = load.downtime_remaining_min.max(0.0);

    let mut stations: Vec<f64> = load
        .in_service_remaining
        .iter()
        .take(capacity)
        .map(|remaining| remaining.max(0.0).max(downtime))
        .collect();
    while stations.len() < capacity {
        stations.push(downtime);
    }

    let committed = load.pending_starts + load.queue_len + load.en_route;
    for _ in 0..committed {
        push_job(&mut stations, service, 0.0);
    }

    // Future demand in chronological order: scenario arrivals integrated in
    // one-minute slices plus anti-herding competitors that will become free at
    // a known time. Likely arrivals are not part of the current queue, so
    // adding them here is not double counting.
    let mut future_arrivals: Vec<(f64, f64)> = Vec::new();
    if let Some(demand) = demand {
        let until = now_min + horizon;
        let mut at = now_min;
        while at < until {
            let next = (at + 1.0).min(until);
            let expected = demand.expected_arrivals(room, at, next);
            if expected > 0.0 {
                future_arrivals.push((at - now_min, expected));
            }
            at = next;
        }
    }
    for arrival in &load.likely_arrivals {
        let weight = arrival.weight.max(0.0);
        if weight > 0.0 && arrival.ready_in_min <= horizon {
            future_arrivals.push((arrival.ready_in_min.max(0.0), weight));
        }
    }
    future_arrivals.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (ready_offset, count) in future_arrivals {
        push_job(&mut stations, service * count, ready_offset);
    }

    let earliest = stations.iter().copied().fold(f64::INFINITY, f64::min);
    (earliest - horizon).max(0.0)
}

fn push_job(stations: &mut [f64], work_min: f64, ready_offset_min: f64) {
    let (idx, _) = stations
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| a.total_cmp(b))
        .expect("capacity is at least one");
    stations[idx] = stations[idx].max(ready_offset_min) + work_min;
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
    projected_wait_at(load, est_service_min, capacity, extra_jobs, 0.0)
}

fn projected_wait_at(
    load: &RoomLoad,
    est_service_min: f64,
    capacity: usize,
    extra_jobs: usize,
    arrive_in_min: f64,
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

    let earliest = stations
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min);
    (earliest - arrive_in_min.max(0.0)).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Package, Room, Scenario, WALK_TIME_MIN};
    use crate::engine::router::ExpectedArrival;
    use crate::scenario::demo_scenario;

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

    #[test]
    fn future_wait_without_demand_matches_decaying_current_wait() {
        let queued = load(2, &[], 0, 0);
        assert!((future_wait_min(&queued, 10.0, 1, None, "a", 0.0, 0.0) - 20.0).abs() < 1e-9);
        assert!((future_wait_min(&queued, 10.0, 1, None, "a", 0.0, 5.0) - 15.0).abs() < 1e-9);
        assert_eq!(future_wait_min(&queued, 10.0, 1, None, "a", 0.0, 20.0), 0.0);
        assert_eq!(future_wait_min(&queued, 10.0, 1, None, "a", 0.0, 50.0), 0.0);
    }

    #[test]
    fn future_wait_grows_during_rush_when_arrivals_outpace_capacity() {
        let demand = ArrivalDemand::from_scenario(&demo_scenario(), 52);
        let empty = RoomLoad::default();
        // Orthopedics: 30% of peak arrivals x 16-minute service exceeds one
        // station, so an idle room still builds a queue as the rush goes on.
        let early = future_wait_min(&empty, 16.0, 1, Some(&demand), "orthopedics", 10.0, 10.0);
        let later = future_wait_min(&empty, 16.0, 1, Some(&demand), "orthopedics", 10.0, 30.0);
        assert!(early > 0.0, "rush should already create pressure at 10 min");
        assert!(later > early, "future wait should grow: {later} vs {early}");
    }

    #[test]
    fn future_wait_fades_to_zero_in_the_lull_and_after_the_last_burst() {
        let demand = ArrivalDemand::from_scenario(&demo_scenario(), 52);
        let queued = load(2, &[], 0, 0);
        let in_lull = future_wait_min(&queued, 10.0, 1, Some(&demand), "ent", 120.0, 0.0);
        assert!((in_lull - 20.0).abs() < 1e-9);
        assert_eq!(
            future_wait_min(&queued, 10.0, 1, Some(&demand), "ent", 120.0, 25.0),
            0.0
        );
        assert_eq!(
            future_wait_min(&queued, 10.0, 1, Some(&demand), "ent", 210.0, 25.0),
            0.0
        );
    }

    #[test]
    fn future_wait_reflects_room_demand_share() {
        // Same service and capacity, but Blood Test is in 90% of packages and
        // ENT in 10%: the popular room accumulates future wait, the quiet one
        // never saturates.
        let scenario = Scenario {
            rooms: vec![
                Room {
                    id: "blood_test".into(),
                    name: "Blood Test".into(),
                    initial_avg_service_min: 9.0,
                    capacity: 1,
                    wing: "test".into(),
                    position: (0.0, 0.0),
                },
                Room {
                    id: "ent".into(),
                    name: "ENT".into(),
                    initial_avg_service_min: 9.0,
                    capacity: 1,
                    wing: "test".into(),
                    position: (1.0, 0.0),
                },
            ],
            dependencies: Vec::new(),
            packages: vec![
                Package {
                    id: "hot".into(),
                    name: "Hot".into(),
                    rooms: vec!["blood_test".into()],
                    weight: 0.9,
                },
                Package {
                    id: "cold".into(),
                    name: "Cold".into(),
                    rooms: vec!["ent".into()],
                    weight: 0.1,
                },
            ],
            walk_time_min: WALK_TIME_MIN,
        };
        let demand = ArrivalDemand::from_scenario(&scenario, 52);
        let empty = RoomLoad::default();
        let blood = future_wait_min(&empty, 9.0, 1, Some(&demand), "blood_test", 0.0, 40.0);
        let ent = future_wait_min(&empty, 9.0, 1, Some(&demand), "ent", 0.0, 40.0);
        assert!(blood > 0.0, "popular room should build a wait");
        assert!(blood > ent, "blood {blood} should exceed ent {ent}");
    }

    #[test]
    fn future_wait_places_likely_arrivals_at_their_ready_time() {
        let mut room = RoomLoad::default();
        room.likely_arrivals = vec![ExpectedArrival {
            ready_in_min: 10.0,
            weight: 1.0,
        }];
        assert_eq!(future_wait_min(&room, 10.0, 1, None, "a", 0.0, 5.0), 0.0);
        assert!((future_wait_min(&room, 10.0, 1, None, "a", 0.0, 10.0) - 10.0).abs() < 1e-9);
        assert!((future_wait_min(&room, 10.0, 1, None, "a", 0.0, 15.0) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn future_wait_respects_downtime_window() {
        let mut room = RoomLoad::default();
        room.downtime_remaining_min = 15.0;
        assert!((future_wait_min(&room, 10.0, 1, None, "a", 0.0, 0.0) - 15.0).abs() < 1e-9);
        assert!((future_wait_min(&room, 10.0, 1, None, "a", 0.0, 10.0) - 5.0).abs() < 1e-9);
        assert_eq!(future_wait_min(&room, 10.0, 1, None, "a", 0.0, 15.0), 0.0);
    }

    #[test]
    fn downtime_that_ends_before_arrival_is_not_charged() {
        let mut room = load(0, &[], 0, 0);
        room.downtime_remaining_min = 3.0;
        // A five-minute walk reaches the room after it reopens.
        assert_eq!(est_wait_at_arrival_min(&room, 10.0, 1, 5.0), 0.0);
        // A two-minute walk still catches the last minute of the outage.
        assert!((est_wait_at_arrival_min(&room, 10.0, 1, 2.0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn shorter_remaining_downtime_has_a_lower_arrival_cost() {
        let mut short = load(0, &[], 0, 0);
        short.downtime_remaining_min = 2.0;
        let mut long = load(0, &[], 0, 0);
        long.downtime_remaining_min = 12.0;
        let cost = |room: &RoomLoad| {
            let walk = 4.0;
            walk + est_wait_at_arrival_min(room, 10.0, 1, walk) + 10.0
        };
        // The short outage ends before the walk does; the long one leaves an
        // eight-minute wait on arrival.
        assert!((cost(&short) - 14.0).abs() < 1e-9);
        assert!((cost(&long) - 22.0).abs() < 1e-9);
        assert!(cost(&short) < cost(&long));
    }

    #[test]
    fn arrival_wait_uses_the_exact_reopen_time() {
        // The in-service patient frees the station at minute 3, but the room
        // stays offline until minute 12: the first usable station is exactly 12.
        let mut room = load(0, &[3.0], 0, 0);
        room.downtime_remaining_min = 12.0;
        assert!((est_wait_at_arrival_min(&room, 10.0, 1, 0.0) - 12.0).abs() < 1e-9);
        assert!((est_wait_at_arrival_min(&room, 10.0, 1, 2.0) - 10.0).abs() < 1e-9);
        assert_eq!(est_wait_at_arrival_min(&room, 10.0, 1, 15.0), 0.0);
        // With no background demand this matches the horizon-aware model.
        assert!(
            (est_wait_at_arrival_min(&room, 10.0, 1, 5.0)
                - future_wait_min(&room, 10.0, 1, None, "a", 0.0, 5.0))
            .abs()
                < 1e-9
        );
    }

    #[test]
    fn arrival_wait_counts_queue_jobs_after_reopening() {
        let mut room = load(1, &[], 0, 0);
        room.downtime_remaining_min = 2.0;
        // The queued patient occupies the station from minute 2 to 12. The
        // newcomer arrives at minute 5 and waits 7.
        assert!((est_wait_at_arrival_min(&room, 10.0, 1, 5.0) - 7.0).abs() < 1e-9);
    }
}
