use crate::domain::{DependencyEdge, Package, Room, Scenario, WALK_TIME_MIN};

pub const DEFAULT_PATIENT_COUNT: usize = 52;
/// The simulated day starts at 08:00, so minute 0 is 08:00 and minute 210 is
/// 11:30. Arrivals cluster into a primary 08:00-10:00 rush and a smaller
/// 10:30-11:30 secondary rush instead of spreading uniformly.
pub const DEFAULT_ARRIVAL_WINDOW_MIN: f64 = 210.0;
pub const PRIMARY_PEAK_START_MIN: f64 = 0.0;
pub const PRIMARY_PEAK_END_MIN: f64 = 120.0;
pub const SECONDARY_PEAK_START_MIN: f64 = 150.0;
pub const SECONDARY_PEAK_END_MIN: f64 = 210.0;
pub const PRIMARY_PEAK_SHARE: f64 = 0.65;

/// Each room has this chance of one unplanned outage per run.
pub const DOWNTIME_PROBABILITY: f64 = 0.15;
pub const DOWNTIME_MIN_MIN: f64 = 10.0;
pub const DOWNTIME_MAX_MIN: f64 = 20.0;

/// Percentage of patients who receive expedited queue treatment.
pub const URGENT_PATIENT_SHARE: f64 = 0.10;

pub const LAB_WING: &str = "Lab wing";
pub const IMAGING_WING: &str = "Imaging wing";
pub const CONSULTATION_WING: &str = "Consultation wing";

pub fn demo_scenario() -> Scenario {
    // Wings are far apart on the hospital map; rooms inside a wing sit within
    // ~2 distance units of each other. With walk_time = 1 + 0.25 * distance
    // this yields 1-2 minute walks inside a wing and 3.5-5 minutes across
    // wings. The entrance (0,0) opens next to the imaging wing, while the lab
    // and consultation wings are the far destinations.
    let rooms = vec![
        Room {
            id: "blood_test".into(),
            name: "Blood Test".into(),
            initial_avg_service_min: 10.0,
            capacity: 3,
            wing: LAB_WING.into(),
            position: (14.0, 9.0),
        },
        Room {
            id: "xray".into(),
            name: "X-Ray".into(),
            initial_avg_service_min: 12.0,
            capacity: 2,
            wing: IMAGING_WING.into(),
            position: (2.5, 2.5),
        },
        Room {
            id: "ultrasound".into(),
            name: "Ultrasound".into(),
            initial_avg_service_min: 12.0,
            capacity: 2,
            wing: IMAGING_WING.into(),
            position: (4.0, 2.5),
        },
        Room {
            id: "ecg".into(),
            name: "ECG".into(),
            initial_avg_service_min: 8.0,
            capacity: 1,
            wing: LAB_WING.into(),
            position: (15.5, 9.0),
        },
        Room {
            id: "cardiology".into(),
            name: "Cardiology".into(),
            initial_avg_service_min: 15.0,
            capacity: 1,
            wing: CONSULTATION_WING.into(),
            position: (6.5, 16.5),
        },
        Room {
            id: "orthopedics".into(),
            name: "Orthopedics".into(),
            initial_avg_service_min: 12.0,
            capacity: 1,
            wing: CONSULTATION_WING.into(),
            position: (8.0, 16.0),
        },
        Room {
            id: "endocrinology".into(),
            name: "Endocrinology".into(),
            initial_avg_service_min: 10.0,
            capacity: 1,
            wing: CONSULTATION_WING.into(),
            position: (9.5, 16.5),
        },
        Room {
            id: "ent".into(),
            name: "ENT".into(),
            initial_avg_service_min: 8.0,
            capacity: 1,
            wing: CONSULTATION_WING.into(),
            position: (7.0, 18.0),
        },
        Room {
            id: "eye_exam".into(),
            name: "Eye Exam".into(),
            initial_avg_service_min: 8.0,
            capacity: 2,
            wing: CONSULTATION_WING.into(),
            position: (9.0, 18.0),
        },
    ];

    let dependencies = vec![
        DependencyEdge::new("blood_test", "endocrinology"),
        DependencyEdge::new("blood_test", "cardiology"),
        DependencyEdge::new("xray", "orthopedics"),
        DependencyEdge::new("ecg", "cardiology"),
    ];

    let packages = vec![
        Package {
            id: "general".into(),
            name: "General Checkup".into(),
            rooms: vec![
                "blood_test".into(),
                "xray".into(),
                "ent".into(),
                "eye_exam".into(),
            ],
            weight: 0.35,
        },
        Package {
            id: "cardiac".into(),
            name: "Cardiac Screen".into(),
            rooms: vec!["blood_test".into(), "ecg".into(), "cardiology".into()],
            weight: 0.15,
        },
        Package {
            id: "joint".into(),
            name: "Joint Pain".into(),
            rooms: vec!["xray".into(), "orthopedics".into()],
            weight: 0.20,
        },
        Package {
            id: "thyroid".into(),
            name: "Thyroid Check".into(),
            rooms: vec![
                "blood_test".into(),
                "ultrasound".into(),
                "endocrinology".into(),
            ],
            weight: 0.20,
        },
        Package {
            id: "premium".into(),
            name: "Full Premium".into(),
            rooms: vec![
                "blood_test".into(),
                "xray".into(),
                "ultrasound".into(),
                "ecg".into(),
                "cardiology".into(),
                "orthopedics".into(),
                "endocrinology".into(),
                "ent".into(),
                "eye_exam".into(),
            ],
            weight: 0.10,
        },
    ];

    Scenario {
        rooms,
        dependencies,
        packages,
        walk_time_min: WALK_TIME_MIN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_scenario_is_valid_and_matches_spec() {
        let scenario = demo_scenario();
        scenario.validate().expect("demo DAG must be acyclic");
        assert_eq!(scenario.rooms.len(), 9);
        assert_eq!(scenario.dependencies.len(), 4);
        assert_eq!(scenario.packages.len(), 5);

        let blood = scenario.room("blood_test").expect("blood test room");
        assert_eq!(blood.capacity, 3);
        let ecg = scenario.room("ecg").expect("ecg room");
        assert_eq!(ecg.capacity, 1);
    }

    #[test]
    fn package_rooms_are_topologically_ordered() {
        let scenario = demo_scenario();
        let graph = scenario.validate().expect("valid scenario");
        for package in &scenario.packages {
            let ordered = graph.topo_order(&package.rooms);
            for edge in &scenario.dependencies {
                if !package.rooms.contains(&edge.from) || !package.rooms.contains(&edge.to) {
                    continue;
                }
                let from = ordered.iter().position(|r| r == &edge.from).unwrap();
                let to = ordered.iter().position(|r| r == &edge.to).unwrap();
                assert!(from < to, "{} must precede {}", edge.from, edge.to);
            }
        }
    }

    #[test]
    fn same_wing_rooms_are_near_and_cross_wing_rooms_are_far() {
        let scenario = demo_scenario();
        for from in &scenario.rooms {
            assert!(!from.wing.is_empty(), "{} needs a wing", from.id);
            for to in &scenario.rooms {
                if from.id == to.id {
                    continue;
                }
                let walk = scenario.walk_time_between(&from.id, &to.id);
                if from.wing == to.wing {
                    assert!(
                        (1.0..=2.0).contains(&walk),
                        "{} -> {} same wing should be 1-2 min, was {walk}",
                        from.id,
                        to.id
                    );
                } else {
                    assert!(
                        (3.0..=5.0).contains(&walk),
                        "{} -> {} across wings should be 3-5 min, was {walk}",
                        from.id,
                        to.id
                    );
                }
            }
        }
        assert!((scenario.walk_time_between("blood_test", "ecg") - 1.375).abs() < 1e-9);
        assert!(scenario.walk_time_between("blood_test", "ecg") < scenario.walk_time_between("blood_test", "cardiology"));
    }
}
