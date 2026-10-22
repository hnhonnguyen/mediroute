use std::collections::{HashMap, VecDeque};
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::domain::{Room, RoomStats, VisitRecord};

pub const EWMA_ALPHA: f64 = 0.3;
pub const MAX_VISIT_LOG: usize = 1000;
const LEARNING_FILE: &str = "learning.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LearningSnapshot {
    #[serde(default)]
    pub room_stats: Vec<RoomStats>,
    #[serde(default)]
    pub visit_log: Vec<VisitRecord>,
}

#[derive(Debug, Clone)]
pub struct Learner {
    stats: HashMap<String, RoomStats>,
    log: VecDeque<VisitRecord>,
    dirty: bool,
}

impl Learner {
    pub fn new(rooms: &[Room], snapshot: Option<&LearningSnapshot>) -> Self {
        let mut stats = HashMap::new();
        for room in rooms {
            let saved = snapshot.and_then(|snapshot| {
                snapshot
                    .room_stats
                    .iter()
                    .find(|stats| stats.room_id == room.id)
            });
            let est_service_min = saved
                .map(|stats| stats.est_service_min)
                .filter(|value| value.is_finite() && *value > 0.0)
                .unwrap_or(room.initial_avg_service_min);
            let est_wait_observed_avg = saved
                .map(|stats| stats.est_wait_observed_avg)
                .filter(|value| value.is_finite() && *value >= 0.0)
                .unwrap_or(0.0);
            stats.insert(
                room.id.clone(),
                RoomStats {
                    room_id: room.id.clone(),
                    est_service_min,
                    est_wait_observed_avg,
                    visit_count: saved.map(|stats| stats.visit_count).unwrap_or(0),
                },
            );
        }

        let mut log = VecDeque::new();
        if let Some(snapshot) = snapshot {
            for record in &snapshot.visit_log {
                log.push_back(record.clone());
            }
            while log.len() > MAX_VISIT_LOG {
                log.pop_front();
            }
        }

        Self {
            stats,
            log,
            dirty: false,
        }
    }

    /// Build a learner for a headless comparison from a fixed snapshot of
    /// estimates, without dragging an unrelated visit log along.
    pub fn from_stats(rooms: &[Room], stats: &HashMap<String, RoomStats>) -> Self {
        let snapshot = LearningSnapshot {
            room_stats: stats.values().cloned().collect(),
            visit_log: Vec::new(),
        };
        Self::new(rooms, Some(&snapshot))
    }

    pub fn stats(&self) -> &HashMap<String, RoomStats> {
        &self.stats
    }

    pub fn recent(&self, limit: usize) -> Vec<VisitRecord> {
        self.log.iter().rev().take(limit).cloned().collect()
    }

    pub fn record(&mut self, record: VisitRecord) {
        if let Some(stats) = self.stats.get_mut(&record.room_id) {
            let actual = record.service_min.max(0.0);
            stats.est_service_min =
                stats.est_service_min * (1.0 - EWMA_ALPHA) + actual * EWMA_ALPHA;
            let wait = record.wait_min.max(0.0);
            stats.est_wait_observed_avg = if stats.visit_count == 0 {
                wait
            } else {
                stats.est_wait_observed_avg * (1.0 - EWMA_ALPHA) + wait * EWMA_ALPHA
            };
            stats.visit_count += 1;
        }
        self.log.push_back(record);
        while self.log.len() > MAX_VISIT_LOG {
            self.log.pop_front();
        }
        self.dirty = true;
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn snapshot(&self) -> LearningSnapshot {
        let mut room_stats: Vec<RoomStats> = self.stats.values().cloned().collect();
        room_stats.sort_by(|a, b| a.room_id.cmp(&b.room_id));
        LearningSnapshot {
            room_stats,
            visit_log: self.log.iter().cloned().collect(),
        }
    }

    pub fn save(&mut self, data_dir: &Path) -> io::Result<()> {
        std::fs::create_dir_all(data_dir)?;
        let path = data_dir.join(LEARNING_FILE);
        let temp = data_dir.join(format!("{LEARNING_FILE}.tmp"));
        let bytes = serde_json::to_vec_pretty(&self.snapshot())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        std::fs::write(&temp, bytes)?;
        std::fs::rename(&temp, &path)?;
        self.dirty = false;
        Ok(())
    }

    pub fn load_snapshot(data_dir: &Path) -> Option<LearningSnapshot> {
        let path = data_dir.join(LEARNING_FILE);
        let raw = std::fs::read_to_string(path).ok()?;
        match serde_json::from_str::<LearningSnapshot>(&raw) {
            Ok(snapshot) => Some(snapshot),
            Err(error) => {
                eprintln!("warning: could not parse learning snapshot: {error}");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::VisitRecord;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn room() -> Room {
        Room {
            id: "blood_test".into(),
            name: "Blood Test".into(),
            initial_avg_service_min: 10.0,
            capacity: 2,
        }
    }

    fn record(service_min: f64, wait_min: f64, sequence: u32) -> VisitRecord {
        VisitRecord {
            patient_id: sequence,
            patient_name: format!("Patient {sequence}"),
            room_id: "blood_test".into(),
            arrival_at_room: 0.0,
            service_start: wait_min,
            service_end: wait_min + service_min,
            wait_min,
            service_min,
        }
    }

    #[test]
    fn ewma_converges_from_seed_toward_true_mean() {
        let rooms = vec![room()];
        let mut learner = Learner::new(&rooms, None);
        assert!((learner.stats()["blood_test"].est_service_min - 10.0).abs() < 1e-9);

        for i in 0..40 {
            learner.record(record(13.0, 4.0, i));
        }

        let stats = &learner.stats()["blood_test"];
        assert!(stats.est_service_min > 12.9, "estimate only reached {}", stats.est_service_min);
        assert!(stats.est_service_min < 13.0);
        assert_eq!(stats.visit_count, 40);
    }

    #[test]
    fn persists_and_reloads_stats_and_visit_log() {
        let rooms = vec![room()];
        let mut learner = Learner::new(&rooms, None);
        learner.record(record(12.0, 3.0, 1));

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "mediroute-learn-test-{}-{unique}",
            std::process::id()
        ));
        learner.save(&dir).expect("save learning snapshot");
        let loaded = Learner::load_snapshot(&dir).expect("load snapshot");
        let restored = Learner::new(&rooms, Some(&loaded));
        assert_eq!(restored.stats()["blood_test"].visit_count, 1);
        assert!((restored.stats()["blood_test"].est_service_min - 10.6).abs() < 1e-9);
        assert_eq!(restored.recent(10).len(), 1);
        std::fs::remove_dir_all(&dir).expect("cleanup temp learning dir");
    }

    #[test]
    fn comparison_learner_starts_from_supplied_stats() {
        let rooms = vec![room()];
        let mut original = Learner::new(&rooms, None);
        for i in 0..10 {
            original.record(record(16.0, 2.0, i));
        }
        let estimate = original.stats()["blood_test"].est_service_min;
        let compare = Learner::from_stats(&rooms, original.stats());
        assert!((compare.stats()["blood_test"].est_service_min - estimate).abs() < 1e-9);
        assert!(compare.recent(10).is_empty());
    }
}
