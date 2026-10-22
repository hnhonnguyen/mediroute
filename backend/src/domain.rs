use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const WALK_TIME_MIN: f64 = 2.0;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Room {
    pub id: String,
    pub name: String,
    pub initial_avg_service_min: f64,
    pub capacity: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoomStats {
    pub room_id: String,
    pub est_service_min: f64,
    pub est_wait_observed_avg: f64,
    pub visit_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DependencyEdge {
    pub from: String,
    pub to: String,
}

impl DependencyEdge {
    pub fn new(from: &str, to: &str) -> Self {
        Self {
            from: from.to_string(),
            to: to.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Package {
    pub id: String,
    pub name: String,
    pub rooms: Vec<String>,
    pub weight: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    pub rooms: Vec<Room>,
    pub dependencies: Vec<DependencyEdge>,
    pub packages: Vec<Package>,
    pub walk_time_min: f64,
}

impl Scenario {
    pub fn room_names(&self) -> HashMap<String, String> {
        self.rooms
            .iter()
            .map(|room| (room.id.clone(), room.name.clone()))
            .collect()
    }

    pub fn room(&self, id: &str) -> Option<&Room> {
        self.rooms.iter().find(|room| room.id == id)
    }

    pub fn validate(&self) -> Result<DependencyGraph, DomainError> {
        if self.packages.is_empty() {
            return Err(DomainError::NoPackages);
        }
        let mut seen = HashSet::new();
        for room in &self.rooms {
            if !seen.insert(room.id.as_str()) {
                return Err(DomainError::DuplicateRoomId(room.id.clone()));
            }
            if room.capacity == 0 {
                return Err(DomainError::InvalidCapacity(room.id.clone()));
            }
            if !room.initial_avg_service_min.is_finite() || room.initial_avg_service_min <= 0.0 {
                return Err(DomainError::InvalidServiceSeed(room.id.clone()));
            }
        }

        for package in &self.packages {
            if package.rooms.is_empty() {
                return Err(DomainError::EmptyPackage(package.id.clone()));
            }
            let mut in_package = HashSet::new();
            for room_id in &package.rooms {
                if !seen.contains(room_id.as_str()) {
                    return Err(DomainError::UnknownPackageRoom {
                        package: package.id.clone(),
                        room: room_id.clone(),
                    });
                }
                if !in_package.insert(room_id.as_str()) {
                    return Err(DomainError::DuplicatePackageRoom {
                        package: package.id.clone(),
                        room: room_id.clone(),
                    });
                }
            }
        }

        DependencyGraph::new(&self.rooms, &self.dependencies)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisitRecord {
    pub patient_id: u32,
    pub patient_name: String,
    pub room_id: String,
    pub arrival_at_room: f64,
    pub service_start: f64,
    pub service_end: f64,
    pub wait_min: f64,
    pub service_min: f64,
}

#[derive(Debug, Clone)]
pub struct Patient {
    pub id: u32,
    pub name: String,
    pub package_id: String,
    pub package_name: String,
    pub arrival_min: f64,
    pub required_rooms: Vec<String>,
    pub done_rooms: Vec<String>,
    pub status: PatientStatus,
    pub current_room: Option<String>,
    pub enroute_since: Option<f64>,
    pub queue_joined_at: Option<f64>,
    pub departed_at: Option<f64>,
    pub visits: Vec<VisitRecord>,
    pub(crate) service_samples: HashMap<String, f64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PatientStatus {
    Arrived,
    EnRoute,
    InQueue,
    InService,
    Departed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyGraph {
    prerequisites: HashMap<String, Vec<String>>,
    dependents: HashMap<String, Vec<String>>,
}

#[derive(Debug, Error)]
pub enum DomainError {
    #[error("duplicate room id: {0}")]
    DuplicateRoomId(String),
    #[error("room has zero capacity: {0}")]
    InvalidCapacity(String),
    #[error("room has an invalid service seed: {0}")]
    InvalidServiceSeed(String),
    #[error("dependency references unknown room: {0}")]
    UnknownRoom(String),
    #[error("room cannot depend on itself: {0}")]
    SelfDependency(String),
    #[error("duplicate dependency: {0} -> {1}")]
    DuplicateDependency(String, String),
    #[error("dependency cycle detected involving room: {0}")]
    CycleDetected(String),
    #[error("package '{package}' references unknown room: {room}")]
    UnknownPackageRoom { package: String, room: String },
    #[error("package '{package}' lists room more than once: {room}")]
    DuplicatePackageRoom { package: String, room: String },
    #[error("package has no rooms: {0}")]
    EmptyPackage(String),
    #[error("scenario has no checkup packages")]
    NoPackages,
}

impl DependencyGraph {
    pub fn new(rooms: &[Room], edges: &[DependencyEdge]) -> Result<Self, DomainError> {
        let room_ids: HashSet<&str> = rooms.iter().map(|room| room.id.as_str()).collect();
        let mut prerequisites: HashMap<String, Vec<String>> = rooms
            .iter()
            .map(|room| (room.id.clone(), Vec::new()))
            .collect();
        let mut dependents: HashMap<String, Vec<String>> = rooms
            .iter()
            .map(|room| (room.id.clone(), Vec::new()))
            .collect();
        let mut seen_edges: HashSet<(&str, &str)> = HashSet::new();

        for edge in edges {
            if !room_ids.contains(edge.from.as_str()) {
                return Err(DomainError::UnknownRoom(edge.from.clone()));
            }
            if !room_ids.contains(edge.to.as_str()) {
                return Err(DomainError::UnknownRoom(edge.to.clone()));
            }
            if edge.from == edge.to {
                return Err(DomainError::SelfDependency(edge.from.clone()));
            }
            if !seen_edges.insert((edge.from.as_str(), edge.to.as_str())) {
                return Err(DomainError::DuplicateDependency(
                    edge.from.clone(),
                    edge.to.clone(),
                ));
            }
            prerequisites
                .get_mut(&edge.to)
                .expect("room validated")
                .push(edge.from.clone());
            dependents
                .get_mut(&edge.from)
                .expect("room validated")
                .push(edge.to.clone());
        }

        // Kahn's algorithm: if every room cannot be emitted, a cycle exists.
        let mut indegree: HashMap<&str, usize> = rooms
            .iter()
            .map(|room| {
                (
                    room.id.as_str(),
                    prerequisites[&room.id].len(),
                )
            })
            .collect();
        let mut ready: VecDeque<&str> = rooms
            .iter()
            .filter(|room| indegree[room.id.as_str()] == 0)
            .map(|room| room.id.as_str())
            .collect();
        let mut emitted = 0usize;
        while let Some(room_id) = ready.pop_front() {
            emitted += 1;
            for dependent in &dependents[room_id] {
                let degree = indegree.get_mut(dependent.as_str()).expect("known room");
                *degree -= 1;
                if *degree == 0 {
                    ready.push_back(dependent);
                }
            }
        }
        if emitted != rooms.len() {
            let offending = rooms
                .iter()
                .find(|room| indegree[room.id.as_str()] > 0)
                .map(|room| room.id.clone())
                .unwrap_or_default();
            return Err(DomainError::CycleDetected(offending));
        }

        Ok(Self {
            prerequisites,
            dependents,
        })
    }

    pub fn prerequisites(&self, room_id: &str) -> &[String] {
        self.prerequisites
            .get(room_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn dependents(&self, room_id: &str) -> &[String] {
        self.dependents
            .get(room_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Stable topological ordering of a package's room list. Unrelated rooms
    /// keep their package-defined relative order.
    pub fn topo_order(&self, required: &[String]) -> Vec<String> {
        let subset: HashSet<&str> = required.iter().map(String::as_str).collect();
        let position: HashMap<&str, usize> = required
            .iter()
            .enumerate()
            .map(|(idx, room)| (room.as_str(), idx))
            .collect();
        let mut indegree: HashMap<&str, usize> = required
            .iter()
            .map(|room| {
                let count = self
                    .prerequisites(room)
                    .iter()
                    .filter(|prereq| subset.contains(prereq.as_str()))
                    .count();
                (room.as_str(), count)
            })
            .collect();
        let mut ready: BinaryHeap<Reverse<(usize, &str)>> = required
            .iter()
            .filter(|room| indegree[room.as_str()] == 0)
            .map(|room| Reverse((position[room.as_str()], room.as_str())))
            .collect();
        let mut ordered = Vec::with_capacity(required.len());
        while let Some(Reverse((_, room_id))) = ready.pop() {
            ordered.push(room_id.to_string());
            for dependent in self.dependents(room_id) {
                if !subset.contains(dependent.as_str()) {
                    continue;
                }
                let degree = indegree
                    .get_mut(dependent.as_str())
                    .expect("room in subset");
                *degree -= 1;
                if *degree == 0 {
                    ready.push(Reverse((position[dependent.as_str()], dependent.as_str())));
                }
            }
        }

        if ordered.len() < required.len() {
            // The global graph is validated acyclic; this is defensive only.
            for room in required {
                if !ordered.contains(room) {
                    ordered.push(room.clone());
                }
            }
        }
        ordered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rooms(ids: &[&str]) -> Vec<Room> {
        ids.iter()
            .map(|id| Room {
                id: id.to_string(),
                name: id.to_string(),
                initial_avg_service_min: 10.0,
                capacity: 1,
            })
            .collect()
    }

    #[test]
    fn detects_dependency_cycles() {
        let rooms = rooms(&["a", "b", "c"]);
        let edges = vec![
            DependencyEdge::new("a", "b"),
            DependencyEdge::new("b", "c"),
            DependencyEdge::new("c", "a"),
        ];
        let result = DependencyGraph::new(&rooms, &edges);
        assert!(matches!(result, Err(DomainError::CycleDetected(_))));
    }

    #[test]
    fn accepts_dag_and_exposes_edges() {
        let rooms = rooms(&["a", "b", "c"]);
        let edges = vec![
            DependencyEdge::new("a", "b"),
            DependencyEdge::new("a", "c"),
        ];
        let graph = DependencyGraph::new(&rooms, &edges).expect("valid DAG");
        assert_eq!(graph.prerequisites("b"), &["a".to_string()]);
        assert_eq!(graph.dependents("a"), &["b".to_string(), "c".to_string()]);
        assert!(graph.prerequisites("a").is_empty());
    }

    #[test]
    fn rejects_self_and_duplicate_dependencies() {
        let rooms = rooms(&["a", "b"]);
        assert!(matches!(
            DependencyGraph::new(&rooms, &[DependencyEdge::new("a", "a")]),
            Err(DomainError::SelfDependency(_))
        ));
        assert!(matches!(
            DependencyGraph::new(
                &rooms,
                &[
                    DependencyEdge::new("a", "b"),
                    DependencyEdge::new("a", "b")
                ]
            ),
            Err(DomainError::DuplicateDependency(_, _))
        ));
    }

    #[test]
    fn topo_order_puts_prerequisites_first_without_disturbing_other_rooms() {
        let rooms = rooms(&["blood", "endo", "us", "ecg"]);
        let graph = DependencyGraph::new(&rooms, &[DependencyEdge::new("blood", "endo")])
            .expect("valid DAG");
        let package = vec![
            "endo".to_string(),
            "us".to_string(),
            "blood".to_string(),
            "ecg".to_string(),
        ];
        assert_eq!(
            graph.topo_order(&package),
            vec![
                "us".to_string(),
                "blood".to_string(),
                "endo".to_string(),
                "ecg".to_string()
            ]
        );
    }
}
