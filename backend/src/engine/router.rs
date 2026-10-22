use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::domain::{DependencyGraph, Room, RoomStats};

use super::congestion;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Strategy {
    Optimized,
    #[serde(alias = "FixedOrder", alias = "fixedorder")]
    FixedOrder,
    #[serde(alias = "ShortestQueue", alias = "shortestqueue")]
    ShortestQueue,
}

impl Strategy {
    pub fn label(self) -> &'static str {
        match self {
            Strategy::Optimized => "Optimized",
            Strategy::FixedOrder => "Fixed Order",
            Strategy::ShortestQueue => "Shortest Queue",
        }
    }

    pub fn all() -> [Strategy; 3] {
        [
            Strategy::Optimized,
            Strategy::FixedOrder,
            Strategy::ShortestQueue,
        ]
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RoomLoad {
    pub queue_len: usize,
    pub in_service: usize,
    pub pending_starts: usize,
    pub en_route: usize,
}

pub struct RouteContext<'a> {
    pub rooms: &'a [Room],
    pub graph: &'a DependencyGraph,
    pub stats: &'a HashMap<String, RoomStats>,
    pub loads: &'a HashMap<String, RoomLoad>,
}

impl RouteContext<'_> {
    pub fn next_room(
        &self,
        strategy: Strategy,
        required: &[String],
        done: &[String],
    ) -> Option<String> {
        let done_set: HashSet<&str> = done.iter().map(String::as_str).collect();
        let required_set: HashSet<&str> = required.iter().map(String::as_str).collect();
        let eligible: Vec<(usize, &String)> = required
            .iter()
            .enumerate()
            .filter(|(_, room)| {
                !done_set.contains(room.as_str()) && self.is_eligible(room, &done_set, &required_set)
            })
            .collect();
        if eligible.is_empty() {
            return None;
        }

        match strategy {
            Strategy::FixedOrder => eligible.first().map(|(_, room)| (*room).clone()),
            Strategy::ShortestQueue => eligible
                .iter()
                .min_by(|(pos_a, room_a), (pos_b, room_b)| {
                    let load_a = self.load(room_a);
                    let load_b = self.load(room_b);
                    load_a
                        .queue_len
                        .cmp(&load_b.queue_len)
                        .then_with(|| pos_a.cmp(pos_b))
                })
                .map(|(_, room)| (*room).clone()),
            Strategy::Optimized => eligible
                .iter()
                .min_by(|(pos_a, room_a), (pos_b, room_b)| {
                    self.compare_optimized(room_a, pos_a, room_b, pos_b, &required_set, &done_set)
                })
                .map(|(_, room)| (*room).clone()),
        }
    }

    fn is_eligible(
        &self,
        room: &str,
        done: &HashSet<&str>,
        required: &HashSet<&str>,
    ) -> bool {
        self.graph.prerequisites(room).iter().all(|prereq| {
            !required.contains(prereq.as_str()) || done.contains(prereq.as_str())
        })
    }

    fn load(&self, room: &str) -> RoomLoad {
        self.loads.get(room).copied().unwrap_or_default()
    }

    fn est_service(&self, room: &str) -> f64 {
        self.stats
            .get(room)
            .map(|stats| stats.est_service_min)
            .or_else(|| {
                self.rooms
                    .iter()
                    .find(|candidate| candidate.id == room)
                    .map(|candidate| candidate.initial_avg_service_min)
            })
            .unwrap_or(10.0)
    }

    fn capacity(&self, room: &str) -> usize {
        self.rooms
            .iter()
            .find(|candidate| candidate.id == room)
            .map(|candidate| candidate.capacity)
            .unwrap_or(1)
    }

    fn unblocks(&self, room: &str, required: &HashSet<&str>, done: &HashSet<&str>) -> usize {
        self.graph
            .dependents(room)
            .iter()
            .filter(|dependent| {
                required.contains(dependent.as_str()) && !done.contains(dependent.as_str())
            })
            .count()
    }

    fn compare_optimized(
        &self,
        room_a: &str,
        pos_a: &usize,
        room_b: &str,
        pos_b: &usize,
        required: &HashSet<&str>,
        done: &HashSet<&str>,
    ) -> Ordering {
        // `min_by` expects Ordering::Less for the preferred room. Ties fall
        // through to unblocking potential, then load, then package position.
        let cost_a = congestion::route_cost(
            &self.load(room_a),
            self.est_service(room_a),
            self.capacity(room_a),
        );
        let cost_b = congestion::route_cost(
            &self.load(room_b),
            self.est_service(room_b),
            self.capacity(room_b),
        );
        let cost_order = cost_a.total_cmp(&cost_b);
        if cost_order != Ordering::Equal {
            return cost_order;
        }

        let unblocks_a = self.unblocks(room_a, required, done);
        let unblocks_b = self.unblocks(room_b, required, done);
        let unblock_order = unblocks_b.cmp(&unblocks_a);
        if unblock_order != Ordering::Equal {
            return unblock_order;
        }

        let load_a = self.load(room_a);
        let load_b = self.load(room_b);
        let enroute_order = load_a.en_route.cmp(&load_b.en_route);
        if enroute_order != Ordering::Equal {
            return enroute_order;
        }
        load_a
            .queue_len
            .cmp(&load_b.queue_len)
            .then_with(|| pos_a.cmp(pos_b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{DependencyEdge, Room};

    fn room(id: &str, seed: f64, capacity: usize) -> Room {
        Room {
            id: id.to_string(),
            name: id.to_string(),
            initial_avg_service_min: seed,
            capacity,
        }
    }

    fn scenario_rooms() -> Vec<Room> {
        vec![
            room("blood", 6.0, 1),
            room("endo", 20.0, 1),
            room("xray", 12.0, 2),
        ]
    }

    fn graph(rooms: &[Room], edges: &[(&str, &str)]) -> DependencyGraph {
        let edges: Vec<DependencyEdge> = edges
            .iter()
            .map(|(from, to)| DependencyEdge::new(from, to))
            .collect();
        DependencyGraph::new(rooms, &edges).expect("valid graph")
    }

    fn stats(rooms: &[Room], overrides: &[(&str, f64)]) -> HashMap<String, RoomStats> {
        rooms
            .iter()
            .map(|room| {
                let estimate = overrides
                    .iter()
                    .find(|(id, _)| *id == room.id)
                    .map(|(_, value)| *value)
                    .unwrap_or(room.initial_avg_service_min);
                (
                    room.id.clone(),
                    RoomStats {
                        room_id: room.id.clone(),
                        est_service_min: estimate,
                        est_wait_observed_avg: 0.0,
                        visit_count: 0,
                    },
                )
            })
            .collect()
    }

    fn loads(entries: &[(&str, usize, usize, usize, usize)]) -> HashMap<String, RoomLoad> {
        entries
            .iter()
            .map(|(id, queue_len, in_service, pending_starts, en_route)| {
                (
                    id.to_string(),
                    RoomLoad {
                        queue_len: *queue_len,
                        in_service: *in_service,
                        pending_starts: *pending_starts,
                        en_route: *en_route,
                    },
                )
            })
            .collect()
    }

    #[test]
    fn dependency_blocks_dependent_room_until_prerequisite_is_done() {
        let rooms = scenario_rooms();
        let graph = graph(&rooms, &[("blood", "endo")]);
        let stats = stats(&rooms, &[]);
        let loads = HashMap::new();
        let ctx = RouteContext {
            rooms: &rooms,
            graph: &graph,
            stats: &stats,
            loads: &loads,
        };
        let required = vec!["blood".into(), "endo".into()];

        assert_eq!(
            ctx.next_room(Strategy::Optimized, &required, &[]),
            Some("blood".to_string())
        );
        assert_eq!(
            ctx.next_room(Strategy::Optimized, &required, &["blood".to_string()]),
            Some("endo".to_string())
        );
    }

    #[test]
    fn optimized_prefers_the_less_congested_eligible_room() {
        let rooms = vec![room("a", 10.0, 1), room("b", 10.0, 1)];
        let graph = graph(&rooms, &[]);
        let stats = stats(&rooms, &[]);
        let loads = loads(&[("a", 5, 0, 0, 0), ("b", 1, 0, 0, 0)]);
        let ctx = RouteContext {
            rooms: &rooms,
            graph: &graph,
            stats: &stats,
            loads: &loads,
        };
        let required = vec!["a".into(), "b".into()];
        assert_eq!(
            ctx.next_room(Strategy::Optimized, &required, &[]),
            Some("b".to_string())
        );
    }

    #[test]
    fn optimized_counts_en_route_patients_to_avoid_herding() {
        let rooms = vec![room("a", 10.0, 1), room("b", 10.0, 1)];
        let graph = graph(&rooms, &[]);
        let stats = stats(&rooms, &[]);
        // Room a looks empty in its queue, but four patients are already
        // walking toward it. Room b has a visible queue of one.
        let loads = loads(&[("a", 0, 0, 0, 4), ("b", 1, 0, 0, 0)]);
        let ctx = RouteContext {
            rooms: &rooms,
            graph: &graph,
            stats: &stats,
            loads: &loads,
        };
        let required = vec!["a".into(), "b".into()];
        assert_eq!(
            ctx.next_room(Strategy::Optimized, &required, &[]),
            Some("b".to_string())
        );
    }

    #[test]
    fn router_uses_learned_service_time_not_config_seed() {
        // Config seeds point the opposite way; learned estimates must win.
        let rooms = vec![room("a", 4.0, 1), room("b", 30.0, 1)];
        let graph = graph(&rooms, &[]);
        let stats = stats(&rooms, &[("a", 25.0), ("b", 5.0)]);
        let loads = HashMap::new();
        let ctx = RouteContext {
            rooms: &rooms,
            graph: &graph,
            stats: &stats,
            loads: &loads,
        };
        let required = vec!["a".into(), "b".into()];
        assert_eq!(
            ctx.next_room(Strategy::Optimized, &required, &[]),
            Some("b".to_string())
        );
    }

    #[test]
    fn fixed_order_follows_package_order_and_ignores_congestion() {
        let rooms = vec![room("a", 10.0, 1), room("b", 10.0, 1)];
        let graph = graph(&rooms, &[]);
        let stats = stats(&rooms, &[]);
        let loads = loads(&[("a", 9, 0, 0, 0), ("b", 0, 0, 0, 0)]);
        let ctx = RouteContext {
            rooms: &rooms,
            graph: &graph,
            stats: &stats,
            loads: &loads,
        };
        let required = vec!["a".into(), "b".into()];
        assert_eq!(
            ctx.next_room(Strategy::FixedOrder, &required, &[]),
            Some("a".to_string())
        );
        assert_eq!(
            ctx.next_room(Strategy::ShortestQueue, &required, &[]),
            Some("b".to_string())
        );
    }

    #[test]
    fn optimized_tie_break_unblocks_dependent_rooms() {
        let rooms = vec![room("a", 10.0, 1), room("b", 10.0, 1), room("c", 10.0, 1)];
        let graph = graph(&rooms, &[("a", "c")]);
        let stats = stats(&rooms, &[]);
        let loads = HashMap::new();
        let ctx = RouteContext {
            rooms: &rooms,
            graph: &graph,
            stats: &stats,
            loads: &loads,
        };
        let required = vec!["a".into(), "b".into(), "c".into()];
        assert_eq!(
            ctx.next_room(Strategy::Optimized, &required, &[]),
            Some("a".to_string())
        );
    }

    #[test]
    fn dependency_only_applies_when_both_rooms_are_required() {
        let rooms = scenario_rooms();
        let graph = graph(&rooms, &[("blood", "endo")]);
        let stats = stats(&rooms, &[]);
        let loads = HashMap::new();
        let ctx = RouteContext {
            rooms: &rooms,
            graph: &graph,
            stats: &stats,
            loads: &loads,
        };

        // Endocrinology alone is fine: the blood-test edge is not relevant.
        let only_endo = vec!["endo".to_string()];
        assert_eq!(
            ctx.next_room(Strategy::Optimized, &only_endo, &[]),
            Some("endo".to_string())
        );

        let both = vec!["blood".to_string(), "endo".to_string()];
        assert_eq!(
            ctx.next_room(
                Strategy::Optimized,
                &both,
                &["blood".to_string(), "endo".to_string()]
            ),
            None
        );
    }
}
