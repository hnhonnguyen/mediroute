use std::collections::HashMap;

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

/// Each room has this chance of one unplanned outage per run. A scheduled
/// outage starts uniformly in `[DOWNTIME_EARLIEST_START_MIN, latest_start)`.
pub const DOWNTIME_PROBABILITY: f64 = 0.15;
pub const DOWNTIME_MIN_MIN: f64 = 10.0;
pub const DOWNTIME_MAX_MIN: f64 = 20.0;
pub const DOWNTIME_EARLIEST_START_MIN: f64 = 15.0;

/// Latest minute at which a seeded outage can begin. This is the published
/// policy window, not a particular RNG draw.
pub fn downtime_latest_start_min() -> f64 {
    (DEFAULT_ARRIVAL_WINDOW_MIN * 0.8 - DOWNTIME_MAX_MIN).max(DOWNTIME_EARLIEST_START_MIN)
}

/// Percentage of patients who receive expedited queue treatment.
pub const URGENT_PATIENT_SHARE: f64 = 0.10;

pub const LAB_WING: &str = "Lab wing";
pub const IMAGING_WING: &str = "Imaging wing";
pub const CONSULTATION_WING: &str = "Consultation wing";

/// A contiguous window with a constant stream of newly arriving patients.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArrivalSegment {
    pub start_min: f64,
    pub end_min: f64,
    /// New patients per minute into the hospital (before room popularity).
    pub rate_per_min: f64,
}

/// Offline demand model for the run: when patients arrive (rush profile) and
/// which fraction of them will need each room (package composition). Both are
/// static, so routing can integrate expected arrivals over any time horizon.
#[derive(Debug, Clone)]
pub struct ArrivalDemand {
    segments: Vec<ArrivalSegment>,
    room_shares: HashMap<String, f64>,
}

impl ArrivalDemand {
    pub fn from_scenario(scenario: &Scenario, patient_count: usize) -> Self {
        let total_weight: f64 = scenario
            .packages
            .iter()
            .map(|package| package.weight.max(0.0))
            .sum();
        let mut room_shares: HashMap<String, f64> = HashMap::new();
        if total_weight > 0.0 {
            for package in &scenario.packages {
                let share = package.weight.max(0.0) / total_weight;
                for room_id in &package.rooms {
                    *room_shares.entry(room_id.clone()).or_default() += share;
                }
            }
        }

        let count = patient_count.max(1) as f64;
        let primary_span = (PRIMARY_PEAK_END_MIN - PRIMARY_PEAK_START_MIN).max(1e-9);
        let secondary_span = (SECONDARY_PEAK_END_MIN - SECONDARY_PEAK_START_MIN).max(1e-9);
        let segments = vec![
            ArrivalSegment {
                start_min: PRIMARY_PEAK_START_MIN,
                end_min: PRIMARY_PEAK_END_MIN,
                rate_per_min: PRIMARY_PEAK_SHARE * count / primary_span,
            },
            ArrivalSegment {
                start_min: SECONDARY_PEAK_START_MIN,
                end_min: SECONDARY_PEAK_END_MIN,
                rate_per_min: (1.0 - PRIMARY_PEAK_SHARE) * count / secondary_span,
            },
        ];

        Self {
            segments,
            room_shares,
        }
    }

    #[cfg(test)]
    pub fn segments(&self) -> &[ArrivalSegment] {
        &self.segments
    }

    /// Share of patients whose package includes `room`.
    pub fn room_share(&self, room: &str) -> f64 {
        self.room_shares
            .get(room)
            .copied()
            .unwrap_or(0.0)
            .clamp(0.0, 1.0)
    }

    /// Expected number of new patients needing `room` between two wall-clock
    /// minutes (minutes since the run started), integrating each window.
    pub fn expected_arrivals(&self, room: &str, from_min: f64, to_min: f64) -> f64 {
        if to_min <= from_min {
            return 0.0;
        }
        let share = self.room_share(room);
        if share <= 0.0 {
            return 0.0;
        }
        self.segments
            .iter()
            .map(|segment| {
                let start = segment.start_min.max(from_min);
                let end = segment.end_min.min(to_min);
                (end - start).max(0.0) * segment.rate_per_min * share
            })
            .sum()
    }
}

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

    #[test]
    fn arrival_demand_conserves_patients_and_respects_the_lull() {
        let scenario = demo_scenario();
        let demand = ArrivalDemand::from_scenario(&scenario, DEFAULT_PATIENT_COUNT);
        let total: f64 = demand
            .segments()
            .iter()
            .map(|segment| segment.rate_per_min * (segment.end_min - segment.start_min))
            .sum();
        assert!((total - DEFAULT_PATIENT_COUNT as f64).abs() < 1e-9);

        // The primary window carries more patients in total (the secondary is
        // denser per minute but only 60 minutes long); the gap and tail are quiet.
        let primary = demand.segments()[0];
        let secondary = demand.segments()[1];
        let primary_total = primary.rate_per_min * (primary.end_min - primary.start_min);
        let secondary_total = secondary.rate_per_min * (secondary.end_min - secondary.start_min);
        assert!(primary_total > secondary_total);
        assert_eq!(demand.expected_arrivals("blood_test", 120.0, 150.0), 0.0);
        assert_eq!(demand.expected_arrivals("blood_test", 210.0, 300.0), 0.0);
        assert!(demand.expected_arrivals("blood_test", 0.0, 20.0) > 0.0);
        assert!(demand.expected_arrivals("blood_test", 150.0, 170.0) > 0.0);
    }

    #[test]
    fn room_shares_follow_package_popularity() {
        let demand = ArrivalDemand::from_scenario(&demo_scenario(), DEFAULT_PATIENT_COUNT);
        // Blood Test appears in four of five packages, ENT in two.
        assert!((demand.room_share("blood_test") - 0.80).abs() < 1e-9);
        assert!((demand.room_share("ent") - 0.45).abs() < 1e-9);
        assert!((demand.room_share("eye_exam") - 0.45).abs() < 1e-9);
        assert!((demand.room_share("ultrasound") - 0.30).abs() < 1e-9);
        assert!(
            demand.expected_arrivals("blood_test", 0.0, 30.0)
                > demand.expected_arrivals("ent", 0.0, 30.0)
        );
    }
}
