use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::domain::{walk_time, DependencyGraph, Room, RoomStats, ENTRANCE_ID, ENTRANCE_POSITION, WALK_TIME_MIN};

use super::congestion;

/// Costs within this relative band count as ties. Round 3 widens the band from
/// 5% to 25% so the distance tie-break can act on moderately congested rooms,
/// not only on near-identical estimates.
const COST_TIE_BAND: f64 = 0.25;

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

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ExpectedArrival {
    /// Minutes from now until this other patient is free to choose their next
    /// room, i.e. when their current commitment ends.
    pub ready_in_min: f64,
    /// Probability-like weight that the patient actually picks this room
    /// (shared across their eligible rooms).
    pub weight: f64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RoomLoad {
    pub queue_len: usize,
    pub in_service: usize,
    pub pending_starts: usize,
    pub en_route: usize,
    /// Remaining service minutes for the cohort currently in service.
    pub in_service_remaining: Vec<f64>,
    /// Other patients expected to compete for this room soon.
    pub likely_arrivals: Vec<ExpectedArrival>,
    /// Minutes until the room reopens after an unplanned outage.
    pub downtime_remaining_min: f64,
}

pub struct RouteContext<'a> {
    pub rooms: &'a [Room],
    pub graph: &'a DependencyGraph,
    pub stats: &'a HashMap<String, RoomStats>,
    pub loads: &'a HashMap<String, RoomLoad>,
    /// Physical location the patient is leaving: `ENTRANCE_ID` or a room id.
    pub current_location: &'a str,
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
        self.loads.get(room).cloned().unwrap_or_default()
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

    fn position(&self, location: &str) -> Option<(f64, f64)> {
        if location == ENTRANCE_ID {
            return Some(ENTRANCE_POSITION);
        }
        self.rooms
            .iter()
            .find(|candidate| candidate.id == location)
            .map(|candidate| candidate.position)
    }

    fn walk_to(&self, room: &str) -> f64 {
        match (self.position(self.current_location), self.position(room)) {
            (Some(from), Some(to)) => walk_time(from, to),
            _ => WALK_TIME_MIN,
        }
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

    /// Longest chain of still-required downstream rooms. Clearing a room with
    /// a deep chain releases more of the patient's critical path at once.
    fn downstream_depth(
        &self,
        room: &str,
        required: &HashSet<&str>,
        done: &HashSet<&str>,
    ) -> usize {
        fn walk(
            graph: &DependencyGraph,
            room: &str,
            required: &HashSet<&str>,
            done: &HashSet<&str>,
            memo: &mut HashMap<String, usize>,
        ) -> usize {
            if let Some(depth) = memo.get(room) {
                return *depth;
            }
            let mut depth = 0;
            for dependent in graph.dependents(room) {
                if !required.contains(dependent.as_str()) || done.contains(dependent.as_str()) {
                    continue;
                }
                depth = depth.max(1 + walk(graph, dependent, required, done, memo));
            }
            memo.insert(room.to_string(), depth);
            depth
        }

        walk(self.graph, room, required, done, &mut HashMap::new())
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
        // `min_by` expects Ordering::Less for the preferred room. Costs within
        // the tie band are resolved by distance and then critical-path rules;
        // clear cost differences still win outright.
        let walk_a = self.walk_to(room_a);
        let walk_b = self.walk_to(room_b);
        let cost_a = congestion::route_cost(
            &self.load(room_a),
            self.est_service(room_a),
            self.capacity(room_a),
            walk_a,
        );
        let cost_b = congestion::route_cost(
            &self.load(room_b),
            self.est_service(room_b),
            self.capacity(room_b),
            walk_b,
        );
        if !costs_within_percent(cost_a, cost_b, COST_TIE_BAND) {
            return cost_a.total_cmp(&cost_b);
        }

        // Near-ties break on walking distance first, keeping the tour inside
        // one wing when congestion is comparable.
        let walk_order = walk_a.total_cmp(&walk_b);
        if walk_order != Ordering::Equal {
            return walk_order;
        }

        let unblocks_a = self.unblocks(room_a, required, done);
        let unblocks_b = self.unblocks(room_b, required, done);
        let unblock_order = unblocks_b.cmp(&unblocks_a);
        if unblock_order != Ordering::Equal {
            return unblock_order;
        }

        let depth_a = self.downstream_depth(room_a, required, done);
        let depth_b = self.downstream_depth(room_b, required, done);
        let depth_order = depth_b.cmp(&depth_a);
        if depth_order != Ordering::Equal {
            return depth_order;
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

fn costs_within_percent(a: f64, b: f64, percent: f64) -> bool {
    let scale = a.abs().max(b.abs());
    if scale <= f64::EPSILON {
        return true;
    }
    (a - b).abs() / scale <= percent
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{DependencyEdge, Room};

    fn room(id: &str, seed: f64, capacity: usize) -> Room {
        room_at(id, seed, capacity, (0.0, 0.0))
    }

    fn room_at(id: &str, seed: f64, capacity: usize, position: (f64, f64)) -> Room {
        Room {
            id: id.to_string(),
            name: id.to_string(),
            initial_avg_service_min: seed,
            capacity,
            wing: "test".into(),
            position,
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
                        ..RoomLoad::default()
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
            current_location: ENTRANCE_ID,
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
            current_location: ENTRANCE_ID,
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
            current_location: ENTRANCE_ID,
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
            current_location: ENTRANCE_ID,
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
            current_location: ENTRANCE_ID,
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
            current_location: ENTRANCE_ID,
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
            current_location: ENTRANCE_ID,
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

    #[test]
    fn optimized_avoids_room_other_patients_will_likely_join() {
        let rooms = vec![room("a", 10.0, 1), room("b", 10.0, 1)];
        let graph = graph(&rooms, &[]);
        let stats = stats(&rooms, &[]);
        let mut loads = loads(&[("a", 0, 0, 0, 0), ("b", 0, 0, 0, 0)]);
        loads.get_mut("a").expect("room a").likely_arrivals = vec![ExpectedArrival {
            ready_in_min: 0.0,
            weight: 1.0,
        }];
        let ctx = RouteContext {
            rooms: &rooms,
            graph: &graph,
            stats: &stats,
            loads: &loads,
            current_location: ENTRANCE_ID,
        };
        let required = vec!["a".into(), "b".into()];
        assert_eq!(
            ctx.next_room(Strategy::Optimized, &required, &[]),
            Some("b".to_string())
        );
    }

    #[test]
    fn tie_break_prefers_deeper_dependency_chain_within_cost_band() {
        // b is marginally cheaper, but a starts a two-room chain (a -> c -> e)
        // while b only unblocks d. Both unblock one immediate dependent.
        let rooms = vec![
            room("a", 10.0, 1),
            room("b", 10.1, 1),
            room("c", 10.0, 1),
            room("d", 10.0, 1),
            room("e", 10.0, 1),
        ];
        let graph = graph(&rooms, &[("a", "c"), ("b", "d"), ("c", "e")]);
        let stats = stats(&rooms, &[]);
        let loads = HashMap::new();
        let ctx = RouteContext {
            rooms: &rooms,
            graph: &graph,
            stats: &stats,
            loads: &loads,
            current_location: ENTRANCE_ID,
        };
        let required = vec![
            "b".to_string(),
            "a".to_string(),
            "c".to_string(),
            "d".to_string(),
            "e".to_string(),
        ];
        assert_eq!(
            ctx.next_room(Strategy::Optimized, &required, &[]),
            Some("a".to_string())
        );
    }

    #[test]
    fn clear_cost_difference_outranks_critical_path_depth() {
        let rooms = vec![
            room("a", 30.0, 1),
            room("b", 10.0, 1),
            room("c", 10.0, 1),
            room("d", 10.0, 1),
        ];
        let graph = graph(&rooms, &[("a", "c"), ("c", "d")]);
        let stats = stats(&rooms, &[]);
        let loads = HashMap::new();
        let ctx = RouteContext {
            rooms: &rooms,
            graph: &graph,
            stats: &stats,
            loads: &loads,
            current_location: ENTRANCE_ID,
        };
        // a is listed first and has the deeper chain, but b is clearly cheaper.
        let required = vec!["a".into(), "b".into(), "c".into(), "d".into()];
        assert_eq!(
            ctx.next_room(Strategy::Optimized, &required, &[]),
            Some("b".to_string())
        );
    }

    #[test]
    fn optimized_prefers_the_nearer_room_when_wait_and_service_match() {
        let rooms = vec![
            room_at("far", 10.0, 1, (20.0, 20.0)),
            room_at("near", 10.0, 1, (3.0, 0.0)),
        ];
        let graph = graph(&rooms, &[]);
        let stats = stats(&rooms, &[]);
        let loads = HashMap::new();
        let ctx = RouteContext {
            rooms: &rooms,
            graph: &graph,
            stats: &stats,
            loads: &loads,
            current_location: ENTRANCE_ID,
        };
        // Same wait and service; the far room is listed first.
        let required = vec!["far".into(), "near".into()];
        assert_eq!(
            ctx.next_room(Strategy::Optimized, &required, &[]),
            Some("near".to_string())
        );
    }

    #[test]
    fn optimized_walk_cost_starts_from_patients_current_room() {
        // The patient is at "west" (6,0). Candidate "next_here" (8,0) is close
        // to them; candidate "near_entrance" (0,6) is closer to the entrance
        // but far from the patient.
        let rooms = vec![
            room_at("west", 10.0, 1, (6.0, 0.0)),
            room_at("next_here", 10.0, 1, (8.0, 0.0)),
            room_at("near_entrance", 10.0, 1, (0.0, 6.0)),
        ];
        let graph = graph(&rooms, &[]);
        let stats = stats(&rooms, &[]);
        let loads = HashMap::new();
        let ctx = RouteContext {
            rooms: &rooms,
            graph: &graph,
            stats: &stats,
            loads: &loads,
            current_location: "west",
        };
        let required = vec!["near_entrance".into(), "next_here".into()];
        assert_eq!(
            ctx.next_room(Strategy::Optimized, &required, &[]),
            Some("next_here".to_string())
        );
    }

    #[test]
    fn baselines_ignore_walk_distance() {
        let rooms = vec![
            room_at("far", 10.0, 1, (20.0, 20.0)),
            room_at("near", 10.0, 1, (3.0, 0.0)),
        ];
        let graph = graph(&rooms, &[]);
        let stats = stats(&rooms, &[]);
        // The near room has a queue; the far room is empty. ShortestQueue must
        // still pick the far room because it never looks at distance.
        let loads = loads(&[("far", 0, 0, 0, 0), ("near", 3, 0, 0, 0)]);
        let ctx = RouteContext {
            rooms: &rooms,
            graph: &graph,
            stats: &stats,
            loads: &loads,
            current_location: ENTRANCE_ID,
        };
        let required = vec!["far".into(), "near".into()];
        assert_eq!(
            ctx.next_room(Strategy::FixedOrder, &required, &[]),
            Some("far".to_string())
        );
        assert_eq!(
            ctx.next_room(Strategy::ShortestQueue, &required, &[]),
            Some("far".to_string())
        );
    }
}
