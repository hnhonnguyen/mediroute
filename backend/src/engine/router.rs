use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::domain::{walk_time, DependencyGraph, Room, RoomStats, ENTRANCE_ID, ENTRANCE_POSITION, WALK_TIME_MIN};
use crate::scenario::{
    downtime_latest_start_min, ArrivalDemand, DOWNTIME_EARLIEST_START_MIN, DOWNTIME_PROBABILITY,
};

use super::congestion;

/// Costs within this relative band count as ties. Round 3 widens the band from
/// 5% to 25% so the distance tie-break can act on moderately congested rooms,
/// not only on near-identical estimates.
const COST_TIE_BAND: f64 = 0.25;

/// Round 4 adds the cheapest projected follow-up visit to the Optimized cost.
/// A sub-1.0 factor keeps the second step informative without letting a busy
/// next room dominate the immediate wait. Tuned on seeds 42/7/123.
const LOOKAHEAD_WEIGHT: f64 = 0.5;

/// Round 16 prices wait variance on top of the expected cost:
/// `cost = walk + wait + service + 0.5 * lookahead + KAPPA * sigma_wait`.
/// KAPPA = 0 reproduces Round 10 exactly. The five-seed sweep picked 0.5:
/// higher values regressed both mean metrics and lost per-seed wins.
const RISK_KAPPA: f64 = 0.5;

// Round 10 keeps outage risk as a late tie-break: the demo policy allows at
// most one outage per room, so a room that has already started its outage is
// risk-free for the rest of the run. Pricing the risk directly into the cost
// shifted 25% tie-band membership and regressed the benchmark, so it only
// settles equal-walk near-ties.

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
    /// Outages this room has already started during the run (finished or
    /// ongoing). The demo policy allows at most one per room, so a positive
    /// count means no further outage is pending.
    pub downtime_episodes: u32,
}

pub struct RouteContext<'a> {
    pub rooms: &'a [Room],
    pub graph: &'a DependencyGraph,
    pub stats: &'a HashMap<String, RoomStats>,
    pub loads: &'a HashMap<String, RoomLoad>,
    /// Physical location the patient is leaving: `ENTRANCE_ID` or a room id.
    pub current_location: &'a str,
    /// Scenario arrival pattern and room popularity, used to price future
    /// congestion. `None` in isolated tests with no background demand.
    pub demand: Option<&'a ArrivalDemand>,
    /// Current simulation clock, needed to align arrival windows.
    pub now_min: f64,
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
        self.walk_between(self.current_location, room)
    }

    fn walk_between(&self, from: &str, to: &str) -> f64 {
        match (self.position(from), self.position(to)) {
            (Some(from), Some(to)) => walk_time(from, to),
            _ => WALK_TIME_MIN,
        }
    }

    fn wait_at(&self, room: &str) -> f64 {
        let load = self.load(room);
        let service = self.est_service(room);
        let capacity = self.capacity(room);
        if load.downtime_remaining_min > 0.0 {
            // The patient is still walking. If the room reopens before they
            // arrive, only the queue ahead of them can make them wait.
            congestion::est_wait_at_arrival_min(
                &load,
                service,
                capacity,
                self.walk_to(room),
            )
        } else {
            congestion::est_wait_min(&load, service, capacity)
        }
    }

    /// Probability that this room's one allowed outage is still ahead, given
    /// that no outage has been observed yet. Uses only the observed episode
    /// history and the published policy (15% chance, start uniform in
    /// `[earliest, latest)`); the seeded schedule itself is never read. A room
    /// that already started its outage is risk-free for the rest of the run.
    fn downtime_risk(&self, room: &str) -> f64 {
        let load = self.load(room);
        if load.downtime_episodes > 0 {
            return 0.0;
        }
        let earliest = DOWNTIME_EARLIEST_START_MIN;
        let latest = downtime_latest_start_min();
        if self.now_min >= latest {
            return 0.0;
        }
        if self.now_min <= earliest {
            return DOWNTIME_PROBABILITY;
        }
        // Bayes: P(scheduled and starting later | no outage so far).
        let window = latest - earliest;
        let future_share = (latest - self.now_min) / window;
        let survived = 1.0 - DOWNTIME_PROBABILITY * (self.now_min - earliest) / window;
        DOWNTIME_PROBABILITY * future_share / survived
    }

    /// Projected wait when arriving at `room` in `arrive_in_min` minutes. The
    /// current station backlog drains at capacity, while expected arrivals
    /// from the scenario demand model add load over the same horizon. During a
    /// rush this can grow instead of decaying; in the lull it falls to zero.
    fn future_wait(&self, room: &str, arrive_in_min: f64) -> f64 {
        congestion::future_wait_min(
            &self.load(room),
            self.est_service(room),
            self.capacity(room),
            self.demand,
            room,
            self.now_min,
            arrive_in_min.max(0.0),
        )
    }

    /// Cheapest projected second step after finishing `room`, or 0 when no
    /// room remains. Eligibility is evaluated with `room` marked done, so the
    /// follow-up set reflects dependencies unlocked by this choice.
    fn lookahead_cost(
        &self,
        room: &str,
        required: &HashSet<&str>,
        done: &HashSet<&str>,
    ) -> f64 {
        let finish_in_min = self.walk_to(room) + self.wait_at(room) + self.est_service(room);
        required
            .iter()
            .copied()
            .filter(|next| *next != room && !done.contains(next))
            .filter(|next| {
                let done_after = |candidate: &str| candidate == room || done.contains(candidate);
                self.graph.prerequisites(next).iter().all(|prereq| {
                    !required.contains(prereq.as_str()) || done_after(prereq.as_str())
                })
            })
            .map(|next| {
                let walk_next = self.walk_between(room, next);
                let wait_next = self.future_wait(next, finish_in_min + walk_next);
                walk_next + wait_next + self.est_service(next)
            })
            .min_by(f64::total_cmp)
            .unwrap_or(0.0)
    }

    fn total_cost(
        &self,
        room: &str,
        required: &HashSet<&str>,
        done: &HashSet<&str>,
    ) -> f64 {
        self.cost_risk(room, required, done, RISK_KAPPA)
    }

    /// Mean cost of this choice: walk + predicted wait + service plus the
    /// half-weighted follow-up.
    fn mean_cost(&self, room: &str, required: &HashSet<&str>, done: &HashSet<&str>) -> f64 {
        self.walk_to(room)
            + self.wait_at(room)
            + self.est_service(room)
            + LOOKAHEAD_WEIGHT * self.lookahead_cost(room, required, done)
    }

    /// Mean-variance cost used by the risk-averse Optimized router. `kappa`
    /// is exposed so tests can verify that zero reproduces the Round 10 cost
    /// and that a positive value penalizes a higher-variance wait.
    fn cost_risk(
        &self,
        room: &str,
        required: &HashSet<&str>,
        done: &HashSet<&str>,
        kappa: f64,
    ) -> f64 {
        self.mean_cost(room, required, done) + kappa * self.wait_sigma(room)
    }

    /// Standard deviation of the wait at `room`, using the queue drain and
    /// arrival components from `congestion::wait_sigma_min`. The horizon runs
    /// from now until the patient is projected to start service.
    fn wait_sigma(&self, room: &str) -> f64 {
        let horizon = self.walk_to(room) + self.wait_at(room);
        congestion::wait_sigma_min(
            &self.load(room),
            self.est_service(room),
            self.capacity(room),
            self.demand,
            room,
            self.now_min,
            horizon,
        )
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
        let cost_a = self.total_cost(room_a, required, done);
        let cost_b = self.total_cost(room_b, required, done);
        if !costs_within_percent(cost_a, cost_b, COST_TIE_BAND) {
            return cost_a.total_cmp(&cost_b);
        }

        // Near-ties break on walking distance first, keeping the tour inside
        // one wing when congestion is comparable.
        let walk_order = walk_a.total_cmp(&walk_b);
        if walk_order != Ordering::Equal {
            return walk_order;
        }

        // Only after distance ties: a room whose one allowed outage has not
        // happened yet still carries a small expected delay. Pricing this
        // before distance regressed the walk time, so it stays a late
        // tie-break rather than an additive cost.
        let risk_a = self.downtime_risk(room_a);
        let risk_b = self.downtime_risk(room_b);
        let risk_order = risk_a.total_cmp(&risk_b);
        if risk_order != Ordering::Equal {
            return risk_order;
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
    use crate::scenario::demo_scenario;

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
            demand: None,
            now_min: 0.0,
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
            demand: None,
            now_min: 0.0,
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
            demand: None,
            now_min: 0.0,
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
            demand: None,
            now_min: 0.0,
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
            demand: None,
            now_min: 0.0,
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
            demand: None,
            now_min: 0.0,
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
            demand: None,
            now_min: 0.0,
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
            demand: None,
            now_min: 0.0,
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
            demand: None,
            now_min: 0.0,
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
            demand: None,
            now_min: 0.0,
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
            demand: None,
            now_min: 0.0,
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
            demand: None,
            now_min: 0.0,
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
            demand: None,
            now_min: 0.0,
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

    #[test]
    fn lookahead_is_zero_when_no_room_remains() {
        let rooms = vec![room("a", 10.0, 1), room("b", 10.0, 1)];
        let graph = graph(&rooms, &[]);
        let stats = stats(&rooms, &[]);
        let loads = loads(&[("a", 4, 0, 0, 0)]);
        let ctx = RouteContext {
            rooms: &rooms,
            graph: &graph,
            stats: &stats,
            loads: &loads,
            current_location: "a",
            demand: None,
            now_min: 0.0,
        };
        let required = vec!["a".to_string(), "b".to_string()];
        let required_set: HashSet<&str> = required.iter().map(String::as_str).collect();
        let done = vec!["a".to_string()];
        let done_set: HashSet<&str> = done.iter().map(String::as_str).collect();

        assert_eq!(ctx.lookahead_cost("b", &required_set, &done_set), 0.0);
        assert_eq!(
            ctx.next_room(Strategy::Optimized, &required, &done),
            Some("b".to_string())
        );
    }

    #[test]
    fn lookahead_wait_decays_with_time_spent_on_the_first_step() {
        let rooms = vec![room("next", 10.0, 1)];
        let graph = graph(&rooms, &[]);
        let stats = stats(&rooms, &[]);
        let loads = loads(&[("next", 2, 0, 0, 0)]);
        let ctx = RouteContext {
            rooms: &rooms,
            graph: &graph,
            stats: &stats,
            loads: &loads,
            current_location: ENTRANCE_ID,
            demand: None,
            now_min: 0.0,
        };

        // Two queued patients at 10 minutes each project a 20-minute wait.
        assert!((ctx.wait_at("next") - 20.0).abs() < 1e-9);
        assert!((ctx.future_wait("next", 5.0) - 15.0).abs() < 1e-9);
        assert_eq!(ctx.future_wait("next", 20.0), 0.0);
        assert_eq!(ctx.future_wait("next", 50.0), 0.0);
    }

    #[test]
    fn lookahead_prices_the_follow_up_at_its_projected_arrival_time() {
        // During the primary rush an overloaded room's projected wait grows
        // with the horizon, so the lookahead must use the exact arrival time,
        // not a linear decay from the current estimate.
        let demand = ArrivalDemand::from_scenario(&demo_scenario(), 52);
        let rooms = vec![
            room_at("first", 5.0, 1, (0.0, 0.0)),
            room_at("orthopedics", 16.0, 1, (5.0, 0.0)),
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
            demand: Some(&demand),
            now_min: 20.0,
        };
        let required = vec!["first".to_string(), "orthopedics".to_string()];
        let required_set: HashSet<&str> = required.iter().map(String::as_str).collect();
        let done_set: HashSet<&str> = HashSet::new();

        let arrive_in = ctx.walk_to("first")
            + ctx.wait_at("first")
            + ctx.est_service("first")
            + ctx.walk_between("first", "orthopedics");
        let expected = ctx.walk_between("first", "orthopedics")
            + ctx.future_wait("orthopedics", arrive_in)
            + ctx.est_service("orthopedics");
        assert!((ctx.lookahead_cost("first", &required_set, &done_set) - expected).abs() < 1e-9);
        assert!(ctx.future_wait("orthopedics", arrive_in) > 0.0);
        assert!(
            ctx.future_wait("orthopedics", arrive_in + 20.0)
                > ctx.future_wait("orthopedics", arrive_in)
        );
    }

    #[test]
    fn demand_model_does_not_change_baseline_choices() {
        let demand = ArrivalDemand::from_scenario(&demo_scenario(), 52);
        let rooms = vec![room("a", 10.0, 1), room("b", 10.0, 1)];
        let graph = graph(&rooms, &[]);
        let stats = stats(&rooms, &[]);
        let loads = loads(&[("a", 0, 0, 0, 0), ("b", 2, 0, 0, 0)]);
        let with_demand = RouteContext {
            rooms: &rooms,
            graph: &graph,
            stats: &stats,
            loads: &loads,
            current_location: ENTRANCE_ID,
            demand: Some(&demand),
            now_min: 30.0,
        };
        let without_demand = RouteContext {
            rooms: &rooms,
            graph: &graph,
            stats: &stats,
            loads: &loads,
            current_location: ENTRANCE_ID,
            demand: None,
            now_min: 30.0,
        };
        let required = vec!["a".to_string(), "b".to_string()];
        for strategy in [Strategy::FixedOrder, Strategy::ShortestQueue] {
            assert_eq!(
                with_demand.next_room(strategy, &required, &[]),
                without_demand.next_room(strategy, &required, &[])
            );
        }
        // Fixed Order follows the package, Shortest Queue follows the queue.
        assert_eq!(
            with_demand.next_room(Strategy::FixedOrder, &required, &[]),
            Some("a".to_string())
        );
        assert_eq!(
            with_demand.next_room(Strategy::ShortestQueue, &required, &[]),
            Some("a".to_string())
        );
    }

    /// X is cheaper right now, but choosing it leaves only the expensive A for
    /// the second step. A costs more now yet unlocks a cheap B, so the two-step
    /// tour through A is cheaper than the greedy tour through X.
    fn two_step_context_rooms() -> (Vec<Room>, DependencyGraph) {
        let rooms = vec![
            room_at("a", 7.0, 1, (0.0, 0.0)),
            room_at("b", 0.1, 1, (2.0, 0.0)),
            room_at("x", 0.8, 1, (20.0, 0.0)),
        ];
        let graph = graph(&rooms, &[("a", "b")]);
        (rooms, graph)
    }

    #[test]
    fn two_step_lookahead_prefers_the_room_that_unlocks_a_cheap_follow_up() {
        let (rooms, graph) = two_step_context_rooms();
        let stats = stats(&rooms, &[]);
        let loads = HashMap::new();
        let ctx = RouteContext {
            rooms: &rooms,
            graph: &graph,
            stats: &stats,
            loads: &loads,
            current_location: ENTRANCE_ID,
            demand: None,
            now_min: 0.0,
        };
        let required = vec!["x".to_string(), "b".to_string(), "a".to_string()];
        let required_set: HashSet<&str> = required.iter().map(String::as_str).collect();
        let done_set: HashSet<&str> = HashSet::new();

        let greedy_x = congestion::route_cost(
            &ctx.load("x"),
            ctx.est_service("x"),
            ctx.capacity("x"),
            ctx.walk_to("x"),
        );
        let greedy_a = congestion::route_cost(
            &ctx.load("a"),
            ctx.est_service("a"),
            ctx.capacity("a"),
            ctx.walk_to("a"),
        );
        assert!((greedy_x - 5.8).abs() < 1e-9, "greedy X was {greedy_x}");
        assert!((greedy_a - 8.0).abs() < 1e-9, "greedy A was {greedy_a}");
        assert!(greedy_x < greedy_a);
        // The one-step gap is wide enough that Round 3 picks X outright.
        assert!(!costs_within_percent(greedy_a, greedy_x, COST_TIE_BAND));

        // A unlocks cheap B as the projected second step; X leaves only A.
        let look_a = ctx.lookahead_cost("a", &required_set, &done_set);
        let look_x = ctx.lookahead_cost("x", &required_set, &done_set);
        assert!((look_a - 1.6).abs() < 1e-9, "lookahead A was {look_a}");
        assert!((look_x - 12.0).abs() < 1e-9, "lookahead X was {look_x}");
        assert!(greedy_a + look_a < greedy_x + look_x);

        // With the half-weighted lookahead, A wins on total cost alone: the
        // gap is outside the tie band, so no tie-break is involved.
        let total_a = ctx.total_cost("a", &required_set, &done_set);
        let total_x = ctx.total_cost("x", &required_set, &done_set);
        assert!((total_a - 8.8).abs() < 1e-9, "total A was {total_a}");
        assert!((total_x - 11.8).abs() < 1e-9, "total X was {total_x}");
        assert!(!costs_within_percent(total_a, total_x, COST_TIE_BAND));

        assert_eq!(
            ctx.next_room(Strategy::Optimized, &required, &[]),
            Some("a".to_string())
        );
    }

    #[test]
    fn lookahead_does_not_change_baseline_choices() {
        let (rooms, graph) = two_step_context_rooms();
        let stats = stats(&rooms, &[]);
        let loads = HashMap::new();
        let ctx = RouteContext {
            rooms: &rooms,
            graph: &graph,
            stats: &stats,
            loads: &loads,
            current_location: ENTRANCE_ID,
            demand: None,
            now_min: 0.0,
        };
        let required = vec!["x".to_string(), "b".to_string(), "a".to_string()];

        // Baseline rules are unchanged: Fixed Order takes package order and
        // Shortest Queue breaks the all-empty tie by order, so both take X.
        assert_eq!(
            ctx.next_room(Strategy::FixedOrder, &required, &[]),
            Some("x".to_string())
        );
        assert_eq!(
            ctx.next_room(Strategy::ShortestQueue, &required, &[]),
            Some("x".to_string())
        );
    }

    fn cost_context<'a>(
        rooms: &'a [Room],
        graph: &'a DependencyGraph,
        stats: &'a HashMap<String, RoomStats>,
        loads: &'a HashMap<String, RoomLoad>,
        now: f64,
    ) -> RouteContext<'a> {
        RouteContext {
            rooms,
            graph,
            stats,
            loads,
            current_location: ENTRANCE_ID,
            demand: None,
            now_min: now,
        }
    }

    fn cost_for(ctx: &RouteContext, room: &str) -> f64 {
        let required = vec![room.to_string()];
        let required_set: HashSet<&str> = required.iter().map(String::as_str).collect();
        ctx.total_cost(room, &required_set, &HashSet::new())
    }

    #[test]
    fn optimized_prices_downtime_at_the_arrival_time() {
        // Both rooms sit at the same position, so walk time is identical.
        let rooms = vec![
            room_at("soon", 10.0, 1, (10.0, 0.0)),
            room_at("late", 10.0, 1, (10.0, 0.0)),
        ];
        let graph = graph(&rooms, &[]);
        let stats = stats(&rooms, &[]);
        let mut loads = HashMap::new();
        for id in ["soon", "late"] {
            loads.insert(
                id.to_string(),
                RoomLoad {
                    // Already down: the one allowed outage is spent, so the
                    // risk term is zero for both rooms.
                    downtime_episodes: 1,
                    ..RoomLoad::default()
                },
            );
        }
        loads
            .get_mut("soon")
            .expect("soon load")
            .downtime_remaining_min = 2.0;
        loads
            .get_mut("late")
            .expect("late load")
            .downtime_remaining_min = 12.0;
        let ctx = cost_context(&rooms, &graph, &stats, &loads, 30.0);

        let walk = ctx.walk_to("soon");
        assert!((walk - 3.5).abs() < 1e-9);
        let soon = cost_for(&ctx, "soon");
        let late = cost_for(&ctx, "late");
        // The short outage ends before the patient arrives: no wait.
        assert!((soon - (walk + 10.0)).abs() < 1e-9);
        // The long outage leaves 8.5 minutes after arrival.
        assert!((late - (walk + 8.5 + 10.0)).abs() < 1e-9);
        assert!(soon < late);
        assert_eq!(ctx.wait_at("soon"), 0.0);
        assert!((ctx.wait_at("late") - 8.5).abs() < 1e-9);
    }

    #[test]
    fn downtime_risk_follows_observed_history_and_the_start_window() {
        let rooms = vec![
            room("risky", 10.0, 1),
            room("spent", 10.0, 1),
            room("other", 30.0, 1),
        ];
        let graph = graph(&rooms, &[]);
        let stats = stats(&rooms, &[]);
        let mut loads = loads(&[("risky", 0, 0, 0, 0), ("spent", 0, 0, 0, 0), ("other", 0, 0, 0, 0)]);
        loads
            .get_mut("spent")
            .expect("spent load")
            .downtime_episodes = 1;

        // Inside the start window, a room with no observed outage still
        // carries a future-outage probability; a spent room does not.
        let early = cost_context(&rooms, &graph, &stats, &loads, 40.0);
        let risky = early.downtime_risk("risky");
        let spent = early.downtime_risk("spent");
        assert!(risky > spent, "unobserved room should carry risk");
        assert!(risky < 0.5, "risk should stay a light nudge");
        // Service length must not reorder equally unobserved rooms; only the
        // observed outage history and the clock can.
        assert!((risky - early.downtime_risk("other")).abs() < 1e-12);

        // After the last possible start neither room has future risk.
        let late = cost_context(&rooms, &graph, &stats, &loads, 200.0);
        assert_eq!(late.downtime_risk("risky"), 0.0);
        assert_eq!(late.downtime_risk("spent"), 0.0);
    }

    #[test]
    fn downtime_risk_only_settles_equal_walk_near_ties() {
        // Both rooms are empty and cost the same, so the tie-break reaches the
        // outage history: the spent room wins even though it is listed second.
        {
            let rooms = vec![
                room_at("risky", 10.0, 1, (10.0, 0.0)),
                room_at("spent", 10.0, 1, (10.0, 0.0)),
            ];
            let graph = graph(&rooms, &[]);
            let stats = stats(&rooms, &[]);
            let mut room_loads = loads(&[("risky", 0, 0, 0, 0), ("spent", 0, 0, 0, 0)]);
            room_loads
                .get_mut("spent")
                .expect("spent load")
                .downtime_episodes = 1;
            let ctx = cost_context(&rooms, &graph, &stats, &room_loads, 40.0);
            let required = vec!["risky".to_string(), "spent".to_string()];
            assert_eq!(
                ctx.next_room(Strategy::Optimized, &required, &[]),
                Some("spent".to_string())
            );
        }

        // Distance still outranks risk: the nearer room wins on a near-tie
        // even if its outage has not happened yet.
        {
            let rooms = vec![
                room_at("near_risky", 10.0, 1, (3.0, 0.0)),
                room_at("far_spent", 10.0, 1, (20.0, 0.0)),
            ];
            let graph = graph(&rooms, &[]);
            let stats = stats(&rooms, &[]);
            let mut room_loads =
                loads(&[("near_risky", 0, 0, 0, 0), ("far_spent", 0, 0, 0, 0)]);
            room_loads
                .get_mut("far_spent")
                .expect("far load")
                .downtime_episodes = 1;
            let ctx = cost_context(&rooms, &graph, &stats, &room_loads, 40.0);
            let required = vec!["far_spent".to_string(), "near_risky".to_string()];
            assert_eq!(
                ctx.next_room(Strategy::Optimized, &required, &[]),
                Some("near_risky".to_string())
            );
        }
    }

    #[test]
    fn downtime_fields_do_not_change_baseline_choices() {
        let rooms = vec![room("down", 10.0, 1), room("up", 10.0, 1)];
        let graph = graph(&rooms, &[]);
        let stats = stats(&rooms, &[]);
        let mut loads = loads(&[("down", 0, 0, 0, 0), ("up", 1, 0, 0, 0)]);
        {
            let down = loads.get_mut("down").expect("down load");
            down.downtime_remaining_min = 15.0;
            down.downtime_episodes = 1;
        }
        let ctx = cost_context(&rooms, &graph, &stats, &loads, 30.0);
        let required = vec!["down".to_string(), "up".to_string()];

        // Fixed Order follows the package and Shortest Queue only counts the
        // queue, even though "down" is offline.
        assert_eq!(
            ctx.next_room(Strategy::FixedOrder, &required, &[]),
            Some("down".to_string())
        );
        assert_eq!(
            ctx.next_room(Strategy::ShortestQueue, &required, &[]),
            Some("down".to_string())
        );
    }

    #[test]
    fn zero_kappa_cost_matches_the_round_ten_formula() {
        let demand = ArrivalDemand::from_scenario(&demo_scenario(), 52);
        let rooms = vec![
            room_at("a", 10.0, 1, (0.0, 0.0)),
            room_at("b", 12.0, 2, (5.0, 0.0)),
        ];
        let graph = graph(&rooms, &[]);
        let stats = stats(&rooms, &[]);
        let room_loads = loads(&[("a", 2, 0, 0, 0), ("b", 0, 0, 0, 0)]);
        let ctx = RouteContext {
            rooms: &rooms,
            graph: &graph,
            stats: &stats,
            loads: &room_loads,
            current_location: ENTRANCE_ID,
            demand: Some(&demand),
            now_min: 30.0,
        };
        let required = vec!["a".to_string(), "b".to_string()];
        let required_set: HashSet<&str> = required.iter().map(String::as_str).collect();
        let done_set = HashSet::new();

        assert!(ctx.wait_sigma("a") > 0.0, "the control context must carry variance");
        for id in ["a", "b"] {
            let r10 = ctx.walk_to(id)
                + ctx.wait_at(id)
                + ctx.est_service(id)
                + LOOKAHEAD_WEIGHT * ctx.lookahead_cost(id, &required_set, &done_set);
            assert_eq!(ctx.cost_risk(id, &required_set, &done_set, 0.0), r10);
            assert_eq!(ctx.mean_cost(id, &required_set, &done_set), r10);
        }
    }

    #[test]
    fn cost_risk_prefers_lower_variance_when_means_are_equal() {
        // Both rooms project a 20-minute wait at the same distance. "steady"
        // has one in-service patient; "risky" runs two stations behind two
        // in-service patients plus three queued jobs, so its chain carries
        // more service-draw variance even though the mean wait matches.
        let rooms = vec![
            room_at("steady", 10.0, 1, (10.0, 0.0)),
            room_at("risky", 10.0, 2, (10.0, 0.0)),
        ];
        let graph = graph(&rooms, &[]);
        let stats = stats(&rooms, &[]);
        let mut room_loads = loads(&[("steady", 0, 1, 0, 0), ("risky", 3, 2, 0, 0)]);
        room_loads
            .get_mut("steady")
            .expect("steady load")
            .in_service_remaining = vec![20.0];
        room_loads
            .get_mut("risky")
            .expect("risky load")
            .in_service_remaining = vec![10.0, 0.0];
        let ctx = cost_context(&rooms, &graph, &stats, &room_loads, 20.0);
        let required = vec!["steady".to_string(), "risky".to_string()];
        let required_set: HashSet<&str> = required.iter().map(String::as_str).collect();
        let done_set = HashSet::new();

        let mean_steady = ctx.mean_cost("steady", &required_set, &done_set);
        let mean_risky = ctx.mean_cost("risky", &required_set, &done_set);
        assert!((mean_steady - mean_risky).abs() < 1e-9);
        assert!(ctx.wait_sigma("steady") < ctx.wait_sigma("risky"));
        assert!(
            ctx.cost_risk("steady", &required_set, &done_set, 1.0)
                < ctx.cost_risk("risky", &required_set, &done_set, 1.0)
        );
        assert_eq!(
            ctx.cost_risk("steady", &required_set, &done_set, 0.0),
            ctx.cost_risk("risky", &required_set, &done_set, 0.0)
        );
    }

    #[test]
    fn risk_term_does_not_change_baseline_choices() {
        let rooms = vec![room("variance_heavy", 10.0, 1), room("calm", 10.0, 1)];
        let graph = graph(&rooms, &[]);
        let stats = stats(&rooms, &[]);
        let mut room_loads = loads(&[("variance_heavy", 3, 0, 0, 0), ("calm", 0, 1, 0, 0)]);
        room_loads
            .get_mut("calm")
            .expect("calm load")
            .in_service_remaining = vec![10.0];
        let ctx = cost_context(&rooms, &graph, &stats, &room_loads, 20.0);
        let required = vec!["variance_heavy".to_string(), "calm".to_string()];

        assert!(ctx.wait_sigma("variance_heavy") > ctx.wait_sigma("calm"));
        assert_eq!(
            ctx.next_room(Strategy::FixedOrder, &required, &[]),
            Some("variance_heavy".to_string())
        );
        assert_eq!(
            ctx.next_room(Strategy::ShortestQueue, &required, &[]),
            Some("calm".to_string())
        );
    }
}
