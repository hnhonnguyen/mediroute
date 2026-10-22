use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::path::Path;

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::Serialize;

use crate::domain::{
    walk_time, DependencyGraph, Package, Patient, PatientPriority, PatientStatus, Room, RoomStats,
    Scenario, VisitRecord, ENTRANCE_ID, ENTRANCE_POSITION, WALK_TIME_MIN,
};
use crate::scenario::{
    DEFAULT_ARRIVAL_WINDOW_MIN, DOWNTIME_MAX_MIN, DOWNTIME_MIN_MIN,
    DOWNTIME_PROBABILITY, PRIMARY_PEAK_END_MIN, PRIMARY_PEAK_SHARE, PRIMARY_PEAK_START_MIN,
    SECONDARY_PEAK_END_MIN, SECONDARY_PEAK_START_MIN, URGENT_PATIENT_SHARE,
};

use super::congestion;
use super::learn::Learner;
use super::router::{ExpectedArrival, RouteContext, RoomLoad, Strategy};
use super::stats::{self, Metrics, RoomUsage};

pub const DEFAULT_SIM_SPEED: f64 = 5.0;
pub const MIN_SIM_SPEED: f64 = 0.1;
pub const MAX_SIM_SPEED: f64 = 60.0;

const FIRST_NAMES: [&str; 24] = [
    "Emma", "Liam", "Olivia", "Noah", "Ava", "Ethan", "Sophia", "Mason", "Isabella", "Lucas",
    "Mia", "Logan", "Amelia", "James", "Harper", "Elijah", "Evelyn", "Aiden", "Linh", "Minh",
    "An", "Bao", "Mai", "Long",
];
const LAST_NAMES: [&str; 20] = [
    "Nguyen", "Tran", "Le", "Pham", "Hoang", "Vu", "Vo", "Dang", "Bui", "Do", "Ho", "Ngo",
    "Duong", "Ly", "Smith", "Johnson", "Garcia", "Chen", "Kim", "Patel",
];

#[derive(Debug, Clone, Serialize)]
pub struct RoomView {
    pub id: String,
    pub name: String,
    pub capacity: usize,
    pub wing: String,
    pub position: (f64, f64),
    pub initial_avg_service_min: f64,
    pub est_service_min: f64,
    pub est_wait_observed_avg: f64,
    pub visit_count: u64,
    pub queue_len: usize,
    pub in_service: usize,
    pub en_route: usize,
    pub est_wait_min: f64,
    pub utilization_pct: f64,
    pub downtime_active: bool,
    pub downtime_until: Option<f64>,
    pub downtime_remaining_min: f64,
}

impl RoomView {
    pub fn idle(rooms: &[Room], stats: &HashMap<String, RoomStats>) -> Vec<RoomView> {
        rooms
            .iter()
            .map(|room| {
                let learned = stats.get(&room.id);
                RoomView {
                    id: room.id.clone(),
                    name: room.name.clone(),
                    capacity: room.capacity,
                    wing: room.wing.clone(),
                    position: room.position,
                    initial_avg_service_min: room.initial_avg_service_min,
                    est_service_min: learned
                        .map(|stats| stats.est_service_min)
                        .unwrap_or(room.initial_avg_service_min),
                    est_wait_observed_avg: learned
                        .map(|stats| stats.est_wait_observed_avg)
                        .unwrap_or(0.0),
                    visit_count: learned.map(|stats| stats.visit_count).unwrap_or(0),
                    queue_len: 0,
                    in_service: 0,
                    en_route: 0,
                    est_wait_min: 0.0,
                    utilization_pct: 0.0,
                    downtime_active: false,
                    downtime_until: None,
                    downtime_remaining_min: 0.0,
                }
            })
            .collect()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PatientVisitView {
    pub room_id: String,
    pub room_name: String,
    pub wait_min: f64,
    pub service_min: f64,
    pub service_end: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PatientView {
    pub id: u32,
    pub name: String,
    pub package_id: String,
    pub package_name: String,
    pub arrival_min: f64,
    pub priority: PatientPriority,
    pub status: PatientStatus,
    pub current_room: Option<String>,
    pub current_room_name: Option<String>,
    pub rooms_done: Vec<String>,
    pub rooms_done_names: Vec<String>,
    pub rooms_remaining: Vec<String>,
    pub rooms_remaining_names: Vec<String>,
    pub rooms_total: usize,
    pub elapsed_min: Option<f64>,
    pub total_time_min: Option<f64>,
    pub visits: Vec<PatientVisitView>,
}

struct InService {
    patient: usize,
    started_at: f64,
    ends_at: f64,
}

struct RoomState {
    queue: VecDeque<usize>,
    in_service: Vec<InService>,
    pending_starts: usize,
    busy_min: f64,
    visits: usize,
    down_until: Option<f64>,
}

#[derive(Debug, Clone)]
enum Event {
    Arrival { patient: usize },
    QueueJoin { patient: usize, room: usize },
    ServiceStart { patient: usize, room: usize },
    ServiceEnd { patient: usize, room: usize },
    Departure { patient: usize },
    RoomDowntimeStart { room: usize, duration_min: f64 },
    RoomDowntimeEnd { room: usize },
}

#[derive(Debug, Clone)]
struct ScheduledEvent {
    at: f64,
    seq: u64,
    event: Event,
}

impl PartialEq for ScheduledEvent {
    fn eq(&self, other: &Self) -> bool {
        self.at.total_cmp(&other.at) == Ordering::Equal && self.seq == other.seq
    }
}

impl Eq for ScheduledEvent {}

impl PartialOrd for ScheduledEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScheduledEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap is a max-heap; reverse both keys so the earliest time,
        // then lowest sequence number, is popped first.
        other
            .at
            .total_cmp(&self.at)
            .then_with(|| other.seq.cmp(&self.seq))
    }
}

pub struct World {
    scenario_rooms: Vec<Room>,
    room_index: HashMap<String, usize>,
    rooms: Vec<RoomState>,
    graph: DependencyGraph,
    strategy: Strategy,
    seed: u64,
    now: f64,
    patients: Vec<Patient>,
    events: BinaryHeap<ScheduledEvent>,
    learner: Learner,
    seq: u64,
    departed: usize,
    finished_at: Option<f64>,
}

impl World {
    pub fn new(
        scenario: &Scenario,
        graph: &DependencyGraph,
        strategy: Strategy,
        seed: u64,
        patient_count: usize,
        learner: Learner,
    ) -> Self {
        let patient_count = patient_count.clamp(1, 200);
        let mut rng = StdRng::seed_from_u64(seed ^ 0x9E37_79B9_7F4A_7C15);
        let room_index: HashMap<String, usize> = scenario
            .rooms
            .iter()
            .enumerate()
            .map(|(idx, room)| (room.id.clone(), idx))
            .collect();
        let rooms = scenario
            .rooms
            .iter()
            .map(|_| RoomState {
                queue: VecDeque::new(),
                in_service: Vec::new(),
                pending_starts: 0,
                busy_min: 0.0,
                visits: 0,
                down_until: None,
            })
            .collect();

        let mut patients = Vec::with_capacity(patient_count);
        for index in 0..patient_count {
            let package = choose_package(&scenario.packages, &mut rng);
            let required_rooms = graph.topo_order(&package.rooms);
            let name = format!(
                "{} {}",
                FIRST_NAMES[rng.gen_range(0..FIRST_NAMES.len())],
                LAST_NAMES[rng.gen_range(0..LAST_NAMES.len())]
            );
            let arrival_min = sample_arrival_min(&mut rng);
            let priority = if rng.gen_bool(URGENT_PATIENT_SHARE) {
                PatientPriority::Urgent
            } else {
                PatientPriority::Normal
            };
            let mut service_samples = HashMap::new();
            for room_id in &required_rooms {
                let mean = true_service_mean(room_id);
                let factor = rng.gen_range(0.8..1.2);
                service_samples.insert(room_id.clone(), (mean * factor).max(0.5));
            }
            patients.push(Patient {
                id: (index + 1) as u32,
                name,
                package_id: package.id.clone(),
                package_name: package.name.clone(),
                arrival_min,
                priority,
                required_rooms,
                done_rooms: Vec::new(),
                status: PatientStatus::Arrived,
                current_room: None,
                current_location: ENTRANCE_ID.into(),
                total_walk_min: 0.0,
                enroute_since: None,
                queue_joined_at: None,
                departed_at: None,
                visits: Vec::new(),
                service_samples,
            });
        }
        patients.sort_by(|a, b| {
            a.arrival_min
                .total_cmp(&b.arrival_min)
                .then_with(|| a.id.cmp(&b.id))
        });

        let mut world = Self {
            scenario_rooms: scenario.rooms.clone(),
            room_index,
            rooms,
            graph: graph.clone(),
            strategy,
            seed,
            now: 0.0,
            patients,
            events: BinaryHeap::new(),
            learner,
            seq: 0,
            departed: 0,
            finished_at: None,
        };
        for idx in 0..world.patients.len() {
            let at = world.patients[idx].arrival_min;
            world.schedule(at, Event::Arrival { patient: idx });
        }
        world.schedule_downtime_windows(&mut rng);
        world
    }

    pub fn strategy(&self) -> Strategy {
        self.strategy
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    pub fn now_min(&self) -> f64 {
        self.now
    }

    pub fn total_patients(&self) -> usize {
        self.patients.len()
    }

    pub fn is_finished(&self) -> bool {
        self.finished_at.is_some()
    }

    pub fn learner(&self) -> &Learner {
        &self.learner
    }

    pub fn persist(&mut self, data_dir: &Path) -> std::io::Result<()> {
        self.learner.save(data_dir)
    }

    pub fn recent_visits(&self, limit: usize) -> Vec<VisitRecord> {
        self.learner.recent(limit)
    }

    /// Advance the virtual clock by `minutes`, processing every event due.
    pub fn advance_minutes(&mut self, minutes: f64) {
        if self.is_finished() {
            if let Some(finished_at) = self.finished_at {
                self.now = finished_at;
            }
            return;
        }
        let target = self.now + minutes.max(0.0);
        while let Some(next_at) = self.events.peek().map(|event| event.at) {
            if next_at > target {
                break;
            }
            let scheduled = self.events.pop().expect("peeked event exists");
            self.now = scheduled.at;
            self.handle_event(scheduled.event);
            if self.is_finished() {
                break;
            }
        }
        self.now = self.finished_at.unwrap_or(target);
    }

    /// Run headless until every patient has departed.
    pub fn run_to_completion(&mut self) {
        let mut processed = 0usize;
        while !self.is_finished() {
            let Some(scheduled) = self.events.pop() else {
                break;
            };
            self.now = scheduled.at;
            self.handle_event(scheduled.event);
            processed += 1;
            if processed > 2_000_000 {
                break;
            }
        }
        if let Some(finished_at) = self.finished_at {
            self.now = finished_at;
        }
    }

    pub fn metrics(&self) -> Metrics {
        let usages: Vec<RoomUsage> = self
            .scenario_rooms
            .iter()
            .zip(self.rooms.iter())
            .map(|(room, state)| RoomUsage {
                room_id: room.id.clone(),
                room_name: room.name.clone(),
                capacity: room.capacity,
                busy_min: state.busy_min,
                visits: state.visits,
            })
            .collect();
        stats::aggregate(&self.patients, &usages, self.now)
    }

    pub fn room_views(&self) -> Vec<RoomView> {
        let loads = self.room_loads(None);
        self.scenario_rooms
            .iter()
            .map(|room| {
                let learned = self.learner.stats().get(&room.id);
                let est_service = learned
                    .map(|stats| stats.est_service_min)
                    .unwrap_or(room.initial_avg_service_min);
                let load = loads.get(&room.id).cloned().unwrap_or_default();
                let state = &self.rooms[self.room_index[&room.id]];
                let downtime_remaining_min = state
                    .down_until
                    .map(|until| (until - self.now).max(0.0))
                    .unwrap_or(0.0);
                let utilization_pct = if self.now > 0.0 {
                    state.busy_min / (self.now * room.capacity as f64).max(1e-9) * 100.0
                } else {
                    0.0
                };
                RoomView {
                    id: room.id.clone(),
                    name: room.name.clone(),
                    capacity: room.capacity,
                    wing: room.wing.clone(),
                    position: room.position,
                    initial_avg_service_min: room.initial_avg_service_min,
                    est_service_min: est_service,
                    est_wait_observed_avg: learned
                        .map(|stats| stats.est_wait_observed_avg)
                        .unwrap_or(0.0),
                    visit_count: learned.map(|stats| stats.visit_count).unwrap_or(0),
                    queue_len: load.queue_len,
                    in_service: load.in_service,
                    en_route: load.en_route,
                    est_wait_min: congestion::est_wait_min(&load, est_service, room.capacity),
                    utilization_pct,
                    downtime_active: downtime_remaining_min > 0.0,
                    downtime_until: state.down_until,
                    downtime_remaining_min,
                }
            })
            .collect()
    }

    pub fn patient_views(&self) -> Vec<PatientView> {
        let names = self
            .scenario_rooms
            .iter()
            .map(|room| (room.id.clone(), room.name.clone()))
            .collect::<HashMap<_, _>>();
        self.patients
            .iter()
            .map(|patient| {
                let rooms_done: Vec<String> = patient
                    .required_rooms
                    .iter()
                    .filter(|room| patient.done_rooms.contains(room))
                    .cloned()
                    .collect();
                let rooms_remaining: Vec<String> = patient
                    .required_rooms
                    .iter()
                    .filter(|room| !patient.done_rooms.contains(room))
                    .cloned()
                    .collect();
                let name_of = |room: &String| names.get(room).cloned().unwrap_or_else(|| room.clone());
                let total_time_min = patient.departed_at.map(|departed| {
                    (departed - patient.arrival_min).max(0.0)
                });
                let elapsed_min = match patient.status {
                    PatientStatus::Arrived => None,
                    PatientStatus::Departed => total_time_min,
                    _ => Some((self.now - patient.arrival_min).max(0.0)),
                };
                PatientView {
                    id: patient.id,
                    name: patient.name.clone(),
                    package_id: patient.package_id.clone(),
                    package_name: patient.package_name.clone(),
                    arrival_min: patient.arrival_min,
                    priority: patient.priority,
                    status: patient.status,
                    current_room: patient.current_room.clone(),
                    current_room_name: patient.current_room.as_ref().map(name_of),
                    rooms_done_names: rooms_done.iter().map(name_of).collect(),
                    rooms_done,
                    rooms_remaining_names: rooms_remaining.iter().map(name_of).collect(),
                    rooms_remaining,
                    rooms_total: patient.required_rooms.len(),
                    elapsed_min,
                    total_time_min,
                    visits: patient
                        .visits
                        .iter()
                        .map(|visit| PatientVisitView {
                            room_id: visit.room_id.clone(),
                            room_name: names
                                .get(&visit.room_id)
                                .cloned()
                                .unwrap_or_else(|| visit.room_id.clone()),
                            wait_min: visit.wait_min,
                            service_min: visit.service_min,
                            service_end: visit.service_end,
                        })
                        .collect(),
                }
            })
            .collect()
    }

    fn schedule(&mut self, at: f64, event: Event) {
        self.seq += 1;
        self.events.push(ScheduledEvent {
            at,
            seq: self.seq,
            event,
        });
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Arrival { patient } => {
                let next = self.next_room(patient);
                match next {
                    Some(room_id) => self.assign_room(patient, room_id),
                    None => self.schedule(self.now, Event::Departure { patient }),
                }
            }
            Event::QueueJoin { patient, room } => {
                if let Some(state) = self.patients.get_mut(patient) {
                    state.status = PatientStatus::InQueue;
                    state.queue_joined_at = Some(self.now);
                }
                self.enqueue(room, patient);
                self.dispatch_room(room);
            }
            Event::ServiceStart { patient, room } => {
                let state = &mut self.rooms[room];
                state.pending_starts = state.pending_starts.saturating_sub(1);
                let room_id = self.scenario_rooms[room].id.clone();
                let duration = self.patients[patient]
                    .service_samples
                    .get(&room_id)
                    .copied()
                    .unwrap_or(self.scenario_rooms[room].initial_avg_service_min)
                    .max(0.5);
                {
                    let patient_state = &mut self.patients[patient];
                    patient_state.status = PatientStatus::InService;
                    patient_state.current_room = Some(room_id);
                }
                let ends_at = self.now + duration;
                self.rooms[room].in_service.push(InService {
                    patient,
                    started_at: self.now,
                    ends_at,
                });
                self.schedule(ends_at, Event::ServiceEnd { patient, room });
            }
            Event::ServiceEnd { patient, room } => {
                let Some(service_pos) = self.rooms[room]
                    .in_service
                    .iter()
                    .position(|entry| entry.patient == patient)
                else {
                    return;
                };
                let service = self.rooms[room].in_service.remove(service_pos);
                let room_id = self.scenario_rooms[room].id.clone();
                let service_min = (self.now - service.started_at).max(0.0);
                let arrival_at_room = self.patients[patient]
                    .queue_joined_at
                    .unwrap_or(service.started_at);
                let wait_min = (service.started_at - arrival_at_room).max(0.0);
                self.rooms[room].busy_min += service_min;
                self.rooms[room].visits += 1;

                let record = VisitRecord {
                    patient_id: self.patients[patient].id,
                    patient_name: self.patients[patient].name.clone(),
                    room_id: room_id.clone(),
                    arrival_at_room,
                    service_start: service.started_at,
                    service_end: self.now,
                    wait_min,
                    service_min,
                };
                {
                    let patient_state = &mut self.patients[patient];
                    patient_state.done_rooms.push(room_id.clone());
                    patient_state.visits.push(record.clone());
                    patient_state.status = PatientStatus::Arrived;
                    patient_state.current_room = None;
                    patient_state.current_location = room_id;
                    patient_state.enroute_since = None;
                    patient_state.queue_joined_at = None;
                }
                self.learner.record(record);
                self.dispatch_room(room);

                let next = self.next_room(patient);
                match next {
                    Some(next_room) => self.assign_room(patient, next_room),
                    None => self.schedule(self.now, Event::Departure { patient }),
                }
            }
            Event::Departure { patient } => {
                if self.patients[patient].status == PatientStatus::Departed {
                    return;
                }
                {
                    let patient_state = &mut self.patients[patient];
                    patient_state.status = PatientStatus::Departed;
                    patient_state.current_room = None;
                    patient_state.departed_at = Some(self.now);
                }
                self.departed += 1;
                if self.departed >= self.patients.len() {
                    self.finished_at = Some(self.now);
                }
            }
            Event::RoomDowntimeStart { room, duration_min } => {
                let until = self.now + duration_min.max(0.0);
                self.rooms[room].down_until = Some(until);
                self.schedule(until, Event::RoomDowntimeEnd { room });
            }
            Event::RoomDowntimeEnd { room } => {
                self.rooms[room].down_until = None;
                self.dispatch_room(room);
            }
        }
    }

    /// Each room independently may take one unplanned 10-20 minute outage.
    fn schedule_downtime_windows(&mut self, rng: &mut StdRng) {
        let latest_start = (DEFAULT_ARRIVAL_WINDOW_MIN * 0.8 - DOWNTIME_MAX_MIN).max(15.0);
        for room in 0..self.scenario_rooms.len() {
            if !rng.gen_bool(DOWNTIME_PROBABILITY) {
                continue;
            }
            let start = rng.gen_range(15.0..latest_start);
            let duration = rng.gen_range(DOWNTIME_MIN_MIN..DOWNTIME_MAX_MIN);
            self.schedule(
                start,
                Event::RoomDowntimeStart {
                    room,
                    duration_min: duration,
                },
            );
        }
    }

    fn enqueue(&mut self, room: usize, patient: usize) {
        if self.patients[patient].priority == PatientPriority::Urgent {
            // Urgent patients go ahead of normals while preserving FIFO order
            // within each priority class.
            let position = self.rooms[room]
                .queue
                .iter()
                .position(|queued| self.patients[*queued].priority == PatientPriority::Normal)
                .unwrap_or_else(|| self.rooms[room].queue.len());
            self.rooms[room].queue.insert(position, patient);
        } else {
            self.rooms[room].queue.push_back(patient);
        }
    }

    fn dispatch_room(&mut self, room: usize) {
        loop {
            if self.rooms[room]
                .down_until
                .is_some_and(|until| self.now < until)
            {
                break;
            }
            let state = &self.rooms[room];
            let busy = state.in_service.len() + state.pending_starts;
            if busy >= self.scenario_rooms[room].capacity {
                break;
            }
            let Some(patient) = self.rooms[room].queue.pop_front() else {
                break;
            };
            self.rooms[room].pending_starts += 1;
            self.schedule(self.now, Event::ServiceStart { patient, room });
        }
    }

    fn walk_time_between(&self, from: &str, to: &str) -> f64 {
        let position = |location: &str| {
            if location == ENTRANCE_ID {
                Some(ENTRANCE_POSITION)
            } else {
                self.scenario_rooms
                    .iter()
                    .find(|room| room.id == location)
                    .map(|room| room.position)
            }
        };
        match (position(from), position(to)) {
            (Some(from), Some(to)) => walk_time(from, to),
            _ => WALK_TIME_MIN,
        }
    }

    fn assign_room(&mut self, patient: usize, room_id: String) {
        let Some(room) = self.room_index.get(&room_id).copied() else {
            return;
        };
        let from = self.patients[patient].current_location.clone();
        let walk_min = self.walk_time_between(&from, &room_id);
        {
            let state = &mut self.patients[patient];
            state.status = PatientStatus::EnRoute;
            state.current_room = Some(room_id);
            state.enroute_since = Some(self.now);
            state.total_walk_min += walk_min;
        }
        self.schedule(
            self.now + walk_min,
            Event::QueueJoin { patient, room },
        );
    }

    fn next_room(&self, patient: usize) -> Option<String> {
        let loads = self.room_loads(Some(patient));
        let patient = &self.patients[patient];
        let context = RouteContext {
            rooms: &self.scenario_rooms,
            graph: &self.graph,
            stats: self.learner.stats(),
            loads: &loads,
            current_location: &patient.current_location,
        };
        context.next_room(self.strategy, &patient.required_rooms, &patient.done_rooms)
    }

    fn room_loads(&self, deciding_patient: Option<usize>) -> HashMap<String, RoomLoad> {
        let mut loads: HashMap<String, RoomLoad> = self
            .scenario_rooms
            .iter()
            .map(|room| (room.id.clone(), RoomLoad::default()))
            .collect();
        let mut in_service_remaining = HashMap::new();
        for (idx, state) in self.rooms.iter().enumerate() {
            let remaining: Vec<f64> = state
                .in_service
                .iter()
                .map(|entry| (entry.ends_at - self.now).max(0.0))
                .collect();
            for entry in &state.in_service {
                in_service_remaining
                    .insert(entry.patient, (entry.ends_at - self.now).max(0.0));
            }
            if let Some(load) = loads.get_mut(&self.scenario_rooms[idx].id) {
                load.queue_len = state.queue.len();
                load.in_service = state.in_service.len();
                load.pending_starts = state.pending_starts;
                load.in_service_remaining = remaining;
                load.downtime_remaining_min = state
                    .down_until
                    .map(|until| (until - self.now).max(0.0))
                    .unwrap_or(0.0);
            }
        }
        for patient in &self.patients {
            if patient.status == PatientStatus::EnRoute {
                if let Some(room) = &patient.current_room {
                    if let Some(load) = loads.get_mut(room) {
                        load.en_route += 1;
                    }
                }
            }
        }
        if let Some(patient) = deciding_patient {
            self.add_likely_arrivals(&mut loads, patient, &in_service_remaining);
        }
        loads
    }

    /// Anti-herding lookahead: estimate which other patients will finish their
    /// current commitment soon and how likely each is to pick a given room.
    /// En-route patients are already counted separately as an immediate load.
    fn add_likely_arrivals(
        &self,
        loads: &mut HashMap<String, RoomLoad>,
        deciding_patient: usize,
        in_service_remaining: &HashMap<usize, f64>,
    ) {
        for (idx, patient) in self.patients.iter().enumerate() {
            if idx == deciding_patient {
                continue;
            }
            let current_room = patient.current_room.as_deref();
            let ready_in_min = match patient.status {
                PatientStatus::InService => {
                    in_service_remaining.get(&idx).copied().unwrap_or(0.0)
                }
                PatientStatus::InQueue => {
                    let Some(room_id) = current_room else {
                        continue;
                    };
                    let est_service = self.estimated_service(room_id);
                    let load = loads.get(room_id).cloned().unwrap_or_default();
                    congestion::est_wait_min(&load, est_service, self.room_capacity(room_id))
                        + est_service
                }
                PatientStatus::EnRoute => {
                    let Some(room_id) = current_room else {
                        continue;
                    };
                    let walk_left = patient
                        .enroute_since
                        .map(|since| {
                            (since
                                + self.walk_time_between(&patient.current_location, room_id)
                                - self.now)
                                .max(0.0)
                        })
                        .unwrap_or(0.0);
                    let est_service = self.estimated_service(room_id);
                    let load = loads.get(room_id).cloned().unwrap_or_default();
                    walk_left
                        + congestion::est_wait_min(&load, est_service, self.room_capacity(room_id))
                        + est_service
                }
                PatientStatus::Arrived => {
                    if patient.arrival_min > self.now {
                        continue;
                    }
                    0.0
                }
                PatientStatus::Departed => continue,
            };

            let done_after: HashSet<&str> = patient
                .done_rooms
                .iter()
                .map(String::as_str)
                .chain(current_room)
                .collect();
            let required: HashSet<&str> = patient
                .required_rooms
                .iter()
                .map(String::as_str)
                .collect();
            let eligible: Vec<&str> = patient
                .required_rooms
                .iter()
                .filter(|room| !done_after.contains(room.as_str()))
                .filter(|room| {
                    self.graph.prerequisites(room).iter().all(|prereq| {
                        !required.contains(prereq.as_str()) || done_after.contains(prereq.as_str())
                    })
                })
                .map(String::as_str)
                .collect();
            if eligible.is_empty() {
                continue;
            }
            let weight = 1.0 / eligible.len() as f64;
            for room_id in eligible {
                if let Some(load) = loads.get_mut(room_id) {
                    load.likely_arrivals.push(ExpectedArrival {
                        ready_in_min,
                        weight,
                    });
                }
            }
        }
    }

    fn estimated_service(&self, room_id: &str) -> f64 {
        self.learner
            .stats()
            .get(room_id)
            .map(|stats| stats.est_service_min)
            .or_else(|| {
                self.scenario_rooms
                    .iter()
                    .find(|room| room.id == room_id)
                    .map(|room| room.initial_avg_service_min)
            })
            .unwrap_or(10.0)
    }

    fn room_capacity(&self, room_id: &str) -> usize {
        self.scenario_rooms
            .iter()
            .find(|room| room.id == room_id)
            .map(|room| room.capacity)
            .unwrap_or(1)
    }
}

/// Two seeded rush-hour bursts with a quiet gap between them. Minute 0 is
/// 08:00, so the peaks cover 08:00-10:00 and 10:30-11:30.
fn sample_arrival_min(rng: &mut StdRng) -> f64 {
    if rng.gen::<f64>() < PRIMARY_PEAK_SHARE {
        rng.gen_range(PRIMARY_PEAK_START_MIN..PRIMARY_PEAK_END_MIN)
    } else {
        rng.gen_range(SECONDARY_PEAK_START_MIN..SECONDARY_PEAK_END_MIN)
    }
}

fn choose_package<'a>(packages: &'a [Package], rng: &mut StdRng) -> &'a Package {
    let total: f64 = packages.iter().map(|package| package.weight.max(0.0)).sum();
    let mut pick = rng.gen_range(0.0..total.max(1e-9));
    for (index, package) in packages.iter().enumerate() {
        let weight = package.weight.max(0.0);
        if pick < weight || index + 1 == packages.len() {
            return package;
        }
        pick -= weight;
    }
    &packages[0]
}

/// The simulator's ground truth intentionally differs from the configured
/// seed, so learners visibly converge toward reality.
pub fn true_service_mean(room_id: &str) -> f64 {
    match room_id {
        "blood_test" => 12.5,
        "xray" => 10.0,
        "ultrasound" => 15.0,
        "ecg" => 11.0,
        "cardiology" => 13.0,
        "orthopedics" => 16.0,
        "endocrinology" => 14.0,
        "ent" => 9.0,
        "eye_exam" => 6.5,
        _ => 10.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Package;
    use crate::engine::learn::Learner;
    use crate::scenario::{demo_scenario, DEFAULT_PATIENT_COUNT};

    fn demo_world(strategy: Strategy, seed: u64, count: usize) -> World {
        let scenario = demo_scenario();
        let graph = scenario.validate().expect("valid demo scenario");
        let learner = Learner::new(&scenario.rooms, None);
        World::new(&scenario, &graph, strategy, seed, count, learner)
    }

    #[test]
    fn all_patients_depart_and_stats_freeze() {
        let mut world = demo_world(Strategy::Optimized, 7, 30);
        world.run_to_completion();
        let metrics = world.metrics();
        assert!(world.is_finished());
        assert_eq!(metrics.total_patients, 30);
        assert_eq!(metrics.completed, 30);
        assert_eq!(metrics.in_hospital, 0);
        assert!(metrics.total_visits > 0);
        let final_time = world.now_min();
        world.advance_minutes(100.0);
        assert_eq!(world.now_min(), final_time);
    }

    #[test]
    fn seeded_runs_are_deterministic() {
        let mut first = demo_world(Strategy::Optimized, 1234, 40);
        let mut second = demo_world(Strategy::Optimized, 1234, 40);
        first.run_to_completion();
        second.run_to_completion();
        assert_eq!(first.now_min(), second.now_min());
        let first_json = serde_json::to_string(&first.patient_views()).expect("serialize");
        let second_json = serde_json::to_string(&second.patient_views()).expect("serialize");
        assert_eq!(first_json, second_json);
        let first_metrics = serde_json::to_string(&first.metrics()).expect("serialize");
        let second_metrics = serde_json::to_string(&second.metrics()).expect("serialize");
        assert_eq!(first_metrics, second_metrics);
    }

    #[test]
    fn optimized_beats_fixed_order_on_a_fixed_seed() {
        let mut optimized = demo_world(Strategy::Optimized, 42, DEFAULT_PATIENT_COUNT);
        let mut fixed = demo_world(Strategy::FixedOrder, 42, DEFAULT_PATIENT_COUNT);
        optimized.run_to_completion();
        fixed.run_to_completion();
        let optimized_metrics = optimized.metrics();
        let fixed_metrics = fixed.metrics();
        assert_eq!(optimized_metrics.completed, DEFAULT_PATIENT_COUNT);
        assert_eq!(fixed_metrics.completed, DEFAULT_PATIENT_COUNT);
        assert!(
            optimized_metrics.avg_total_min <= fixed_metrics.avg_total_min,
            "optimized avg {} should not exceed fixed order avg {}",
            optimized_metrics.avg_total_min,
            fixed_metrics.avg_total_min
        );
    }

    #[test]
    fn single_room_packages_complete() {
        let scenario = Scenario {
            rooms: vec![Room {
                id: "only".into(),
                name: "Only Room".into(),
                initial_avg_service_min: 5.0,
                capacity: 1,
                wing: "test".into(),
                position: (0.0, 0.0),
            }],
            dependencies: Vec::new(),
            packages: vec![Package {
                id: "single".into(),
                name: "Single".into(),
                rooms: vec!["only".into()],
                weight: 1.0,
            }],
            walk_time_min: WALK_TIME_MIN,
        };
        let graph = scenario.validate().expect("valid scenario");
        let learner = Learner::new(&scenario.rooms, None);
        let mut world = World::new(&scenario, &graph, Strategy::Optimized, 9, 5, learner);
        world.run_to_completion();
        let metrics = world.metrics();
        assert_eq!(metrics.completed, 5);
        assert_eq!(metrics.total_visits, 5);
    }

    #[test]
    fn learning_starts_at_seed_and_moves_toward_observed_service() {
        let mut world = demo_world(Strategy::Optimized, 5, 20);
        let before = world
            .learner()
            .stats()
            .get("ecg")
            .expect("ecg stats")
            .est_service_min;
        world.run_to_completion();
        let stats = world.learner().stats().get("ecg").expect("ecg stats");
        assert!(stats.visit_count > 0);
        assert!(
            (stats.est_service_min - true_service_mean("ecg")).abs()
                < (before - true_service_mean("ecg")).abs(),
            "estimate {} (seed {}) should move toward true mean {}",
            stats.est_service_min,
            before,
            true_service_mean("ecg")
        );
    }

    #[test]
    fn patients_arrive_in_two_rush_hour_bursts() {
        let world = demo_world(Strategy::Optimized, 42, DEFAULT_PATIENT_COUNT);
        let arrivals: Vec<f64> = world.patients.iter().map(|p| p.arrival_min).collect();
        let primary = arrivals
            .iter()
            .filter(|at| **at < PRIMARY_PEAK_END_MIN)
            .count();
        let secondary = arrivals
            .iter()
            .filter(|at| **at >= SECONDARY_PEAK_START_MIN)
            .count();
        let quiet_gap = arrivals
            .iter()
            .filter(|at| **at >= PRIMARY_PEAK_END_MIN && **at < SECONDARY_PEAK_START_MIN)
            .count();

        assert_eq!(primary + secondary, arrivals.len());
        assert_eq!(quiet_gap, 0, "no arrivals should land in the lull");
        assert!(primary > 0 && secondary > 0);
        assert!(
            primary > secondary,
            "primary burst should carry more patients ({primary} vs {secondary})"
        );
    }

    #[test]
    fn urgent_patients_jump_ahead_of_normal_queue() {
        let mut world = demo_world(Strategy::Optimized, 3, 4);
        let priorities = [
            PatientPriority::Normal,
            PatientPriority::Urgent,
            PatientPriority::Normal,
            PatientPriority::Urgent,
        ];
        for (idx, priority) in priorities.iter().enumerate() {
            world.patients[idx].priority = *priority;
        }
        for idx in 0..4 {
            world.enqueue(0, idx);
        }
        let order: Vec<usize> = world.rooms[0].queue.iter().copied().collect();
        assert_eq!(order, vec![1, 3, 0, 2]);
    }

    #[test]
    fn urgent_and_normal_wait_metrics_are_reported() {
        let mut world = demo_world(Strategy::Optimized, 42, DEFAULT_PATIENT_COUNT);
        let urgent = world
            .patients
            .iter()
            .filter(|patient| patient.priority == PatientPriority::Urgent)
            .count();
        assert!(urgent > 0, "seeded run should include urgent patients");
        world.run_to_completion();
        let metrics = world.metrics();
        assert!(metrics.avg_wait_urgent_min.is_finite());
        assert!(metrics.p90_wait_urgent_min.is_finite());
        assert!(metrics.avg_wait_normal_min.is_finite());
        assert!(metrics.p90_wait_normal_min.is_finite());
        assert!(
            metrics.p90_wait_urgent_min <= metrics.p90_wait_normal_min
                || metrics.avg_wait_urgent_min <= metrics.avg_wait_normal_min,
            "urgent waits should not be worse than normal waits"
        );
    }

    #[test]
    fn downtime_holds_queue_and_is_visible_in_room_views() {
        let mut world = demo_world(Strategy::Optimized, 11, 6);
        world.rooms[0].queue.push_back(0);
        world.handle_event(Event::RoomDowntimeStart {
            room: 0,
            duration_min: 15.0,
        });
        assert_eq!(world.rooms[0].down_until, Some(world.now_min() + 15.0));

        world.dispatch_room(0);
        assert_eq!(world.rooms[0].queue.len(), 1);
        assert_eq!(world.rooms[0].pending_starts, 0);

        let view = world
            .room_views()
            .into_iter()
            .find(|view| view.id == world.scenario_rooms[0].id)
            .expect("room view");
        assert!(view.downtime_active);
        assert!((view.downtime_remaining_min - 15.0).abs() < 1e-9);
        assert!((view.est_wait_min - 15.0).abs() < 1e-9);

        world.handle_event(Event::RoomDowntimeEnd { room: 0 });
        assert!(world.rooms[0].down_until.is_none());
        assert!(world.rooms[0].queue.is_empty());
        assert_eq!(world.rooms[0].pending_starts, 1);
    }

    #[test]
    fn downtime_lets_in_service_finish_but_holds_new_starts() {
        let mut world = demo_world(Strategy::Optimized, 5, 4);
        world.rooms[0].in_service.push(InService {
            patient: 0,
            started_at: 0.0,
            ends_at: 10.0,
        });
        world.rooms[0].queue.push_back(1);
        world.handle_event(Event::RoomDowntimeStart {
            room: 0,
            duration_min: 15.0,
        });

        // The running visit completes normally even while the room is offline.
        world.handle_event(Event::ServiceEnd { patient: 0, room: 0 });
        assert!(world.rooms[0].in_service.is_empty());
        assert!(world.patients[0]
            .visits
            .iter()
            .any(|visit| visit.room_id == "blood_test"));
        assert_eq!(world.rooms[0].queue.len(), 1);

        // The queued patient is only dispatched after the room reopens.
        world.handle_event(Event::RoomDowntimeEnd { room: 0 });
        assert!(world.rooms[0].queue.is_empty());
        assert_eq!(world.rooms[0].pending_starts, 1);
    }

    #[test]
    fn en_route_uses_distance_based_walk_time() {
        let scenario = demo_scenario();
        let mut world = demo_world(Strategy::Optimized, 42, 1);
        world.events.clear();
        world.patients[0].current_location = "blood_test".into();
        let started_at = world.now;
        world.assign_room(0, "ecg".into());

        let expected = scenario.walk_time_between("blood_test", "ecg");
        let cardiology = scenario.walk_time_between("blood_test", "cardiology");
        assert!(expected < cardiology, "same-wing ECG should be closer than Cardiology");
        let join_at = world
            .events
            .iter()
            .find_map(|event| match &event.event {
                Event::QueueJoin { patient: 0, .. } => Some(event.at),
                _ => None,
            })
            .expect("queue join was scheduled");
        assert!(
            (join_at - (started_at + expected)).abs() < 1e-9,
            "queue join at {join_at}, expected walk {expected}"
        );
        assert!((world.patients[0].total_walk_min - expected).abs() < 1e-9);
    }

    #[test]
    fn patient_location_starts_at_entrance_and_tracks_completed_rooms() {
        let mut world = demo_world(Strategy::Optimized, 5, 2);
        assert_eq!(world.patients[0].current_location, ENTRANCE_ID);

        world.rooms[0].in_service.push(InService {
            patient: 0,
            started_at: 0.0,
            ends_at: 0.0,
        });
        let room_id = world.scenario_rooms[0].id.clone();
        world.handle_event(Event::ServiceEnd { patient: 0, room: 0 });
        assert_eq!(world.patients[0].current_location, room_id);
    }

    #[test]
    fn metrics_report_average_patient_walk_time() {
        let mut world = demo_world(Strategy::Optimized, 42, DEFAULT_PATIENT_COUNT);
        world.run_to_completion();
        let metrics = world.metrics();
        assert!(metrics.avg_walk_min > 0.0);
        let manual = world
            .patients
            .iter()
            .map(|patient| patient.total_walk_min)
            .sum::<f64>()
            / world.patients.len() as f64;
        assert!((metrics.avg_walk_min - manual).abs() < 1e-9);
    }
}
