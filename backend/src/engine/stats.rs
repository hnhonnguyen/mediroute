use serde::Serialize;

use crate::domain::{Patient, PatientPriority, PatientStatus, ENTRANCE_ID};

#[derive(Debug, Clone, Serialize)]
pub struct RoomUtilization {
    pub room_id: String,
    pub room_name: String,
    pub capacity: usize,
    pub utilization_pct: f64,
    pub visits: usize,
    pub avg_service_min: f64,
    pub total_busy_min: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Metrics {
    pub total_patients: usize,
    pub arrived: usize,
    pub completed: usize,
    pub in_hospital: usize,
    pub avg_total_min: f64,
    pub avg_wait_min: f64,
    pub p90_total_min: f64,
    pub p90_wait_min: f64,
    pub avg_wait_urgent_min: f64,
    pub p90_wait_urgent_min: f64,
    pub avg_wait_normal_min: f64,
    pub p90_wait_normal_min: f64,
    pub avg_walk_min: f64,
    pub total_visits: usize,
    pub room_utilization: Vec<RoomUtilization>,
}

pub struct RoomUsage {
    pub room_id: String,
    pub room_name: String,
    pub capacity: usize,
    pub busy_min: f64,
    pub visits: usize,
}

impl Metrics {
    pub fn empty() -> Self {
        Self {
            total_patients: 0,
            arrived: 0,
            completed: 0,
            in_hospital: 0,
            avg_total_min: 0.0,
            avg_wait_min: 0.0,
            p90_total_min: 0.0,
            p90_wait_min: 0.0,
            avg_wait_urgent_min: 0.0,
            p90_wait_urgent_min: 0.0,
            avg_wait_normal_min: 0.0,
            p90_wait_normal_min: 0.0,
            avg_walk_min: 0.0,
            total_visits: 0,
            room_utilization: Vec::new(),
        }
    }
}

pub fn aggregate(patients: &[Patient], rooms: &[RoomUsage], now_min: f64) -> Metrics {
    let total_patients = patients.len();
    let arrived = patients
        .iter()
        .filter(|patient| patient.status != PatientStatus::Arrived)
        .count();
    let total_times: Vec<f64> = patients
        .iter()
        .filter_map(|patient| {
            patient
                .departed_at
                .map(|departed| (departed - patient.arrival_min).max(0.0))
        })
        .collect();
    let completed = total_times.len();
    let waits: Vec<f64> = patients
        .iter()
        .flat_map(|patient| patient.visits.iter().map(|visit| visit.wait_min))
        .collect();
    let total_visits = waits.len();
    let total_walks: Vec<f64> = patients
        .iter()
        .filter(|patient| patient.departed_at.is_some())
        .map(|patient| patient.total_walk_min)
        .collect();

    let waits_for = |priority: PatientPriority| -> Vec<f64> {
        patients
            .iter()
            .filter(|patient| patient.priority == priority)
            .flat_map(|patient| patient.visits.iter().map(|visit| visit.wait_min))
            .collect()
    };
    let urgent_waits = waits_for(PatientPriority::Urgent);
    let normal_waits = waits_for(PatientPriority::Normal);

    let avg_total_min = mean(&total_times);
    let avg_wait_min = mean(&waits);
    let p90_total_min = percentile_90(&total_times);
    let p90_wait_min = percentile_90(&waits);

    let room_utilization = rooms
        .iter()
        .map(|room| {
            let capacity_minutes = (now_min.max(0.0) * room.capacity as f64).max(1e-9);
            RoomUtilization {
                room_id: room.room_id.clone(),
                room_name: room.room_name.clone(),
                capacity: room.capacity,
                utilization_pct: room.busy_min / capacity_minutes * 100.0,
                visits: room.visits,
                avg_service_min: if room.visits > 0 {
                    room.busy_min / room.visits as f64
                } else {
                    0.0
                },
                total_busy_min: room.busy_min,
            }
        })
        .collect();

    Metrics {
        total_patients,
        arrived,
        completed,
        in_hospital: arrived.saturating_sub(completed),
        avg_total_min,
        avg_wait_min,
        p90_total_min,
        p90_wait_min,
        avg_wait_urgent_min: mean(&urgent_waits),
        p90_wait_urgent_min: percentile_90(&urgent_waits),
        avg_wait_normal_min: mean(&normal_waits),
        p90_wait_normal_min: percentile_90(&normal_waits),
        avg_walk_min: mean(&total_walks),
        total_visits,
        room_utilization,
    }
}

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

fn percentile_90(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let rank = ((sorted.len() as f64) * 0.9).ceil() as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::domain::VisitRecord;

    use super::*;

    fn patient(priority: PatientPriority, id: u32, waits: &[f64]) -> Patient {
        Patient {
            id,
            name: format!("Patient {id}"),
            package_id: "pkg".into(),
            package_name: "Package".into(),
            arrival_min: 0.0,
            priority,
            required_rooms: Vec::new(),
            done_rooms: Vec::new(),
            status: PatientStatus::Departed,
            current_room: None,
            current_location: ENTRANCE_ID.into(),
            total_walk_min: 0.0,
            enroute_since: None,
            queue_joined_at: None,
            departed_at: Some(30.0),
            visits: waits
                .iter()
                .enumerate()
                .map(|(index, wait)| VisitRecord {
                    patient_id: id,
                    patient_name: format!("Patient {id}"),
                    room_id: format!("room_{index}"),
                    arrival_at_room: 0.0,
                    service_start: *wait,
                    service_end: wait + 1.0,
                    wait_min: *wait,
                    service_min: 1.0,
                })
                .collect(),
            service_samples: HashMap::new(),
        }
    }

    #[test]
    fn percentile_uses_nearest_rank() {
        let values: Vec<f64> = (1..=10).map(f64::from).collect();
        assert_eq!(percentile_90(&values), 9.0);
        assert_eq!(percentile_90(&[42.0]), 42.0);
        assert_eq!(percentile_90(&[]), 0.0);
    }

    #[test]
    fn priority_wait_metrics_are_reported_separately() {
        let patients = vec![
            patient(PatientPriority::Urgent, 1, &[1.0, 2.0]),
            patient(PatientPriority::Normal, 2, &[10.0, 20.0]),
        ];
        let metrics = aggregate(&patients, &[], 30.0);
        assert!((metrics.avg_wait_urgent_min - 1.5).abs() < 1e-9);
        assert!((metrics.p90_wait_urgent_min - 2.0).abs() < 1e-9);
        assert!((metrics.avg_wait_normal_min - 15.0).abs() < 1e-9);
        assert!((metrics.p90_wait_normal_min - 20.0).abs() < 1e-9);
        assert!((metrics.avg_wait_min - 8.25).abs() < 1e-9);
    }

    #[test]
    fn average_walk_time_is_reported_per_patient() {
        let mut first = patient(PatientPriority::Normal, 1, &[1.0]);
        first.total_walk_min = 4.0;
        let mut second = patient(PatientPriority::Normal, 2, &[2.0]);
        second.total_walk_min = 8.0;
        let metrics = aggregate(&[first, second], &[], 30.0);
        assert!((metrics.avg_walk_min - 6.0).abs() < 1e-9);
    }
}
