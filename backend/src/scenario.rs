use crate::domain::{DependencyEdge, Package, Room, Scenario, WALK_TIME_MIN};

pub const DEFAULT_PATIENT_COUNT: usize = 52;
pub const DEFAULT_ARRIVAL_WINDOW_MIN: f64 = 88.0;

pub fn demo_scenario() -> Scenario {
    let rooms = vec![
        Room {
            id: "blood_test".into(),
            name: "Blood Test".into(),
            initial_avg_service_min: 10.0,
            capacity: 3,
        },
        Room {
            id: "xray".into(),
            name: "X-Ray".into(),
            initial_avg_service_min: 12.0,
            capacity: 2,
        },
        Room {
            id: "ultrasound".into(),
            name: "Ultrasound".into(),
            initial_avg_service_min: 12.0,
            capacity: 2,
        },
        Room {
            id: "ecg".into(),
            name: "ECG".into(),
            initial_avg_service_min: 8.0,
            capacity: 1,
        },
        Room {
            id: "cardiology".into(),
            name: "Cardiology".into(),
            initial_avg_service_min: 15.0,
            capacity: 1,
        },
        Room {
            id: "orthopedics".into(),
            name: "Orthopedics".into(),
            initial_avg_service_min: 12.0,
            capacity: 1,
        },
        Room {
            id: "endocrinology".into(),
            name: "Endocrinology".into(),
            initial_avg_service_min: 10.0,
            capacity: 1,
        },
        Room {
            id: "ent".into(),
            name: "ENT".into(),
            initial_avg_service_min: 8.0,
            capacity: 1,
        },
        Room {
            id: "eye_exam".into(),
            name: "Eye Exam".into(),
            initial_avg_service_min: 8.0,
            capacity: 2,
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
}
