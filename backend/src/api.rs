use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::RwLock;

use crate::domain::{DependencyGraph, RoomStats, Scenario, VisitRecord};
use crate::engine::learn::{Learner, LearningSnapshot};
use crate::engine::router::Strategy;
use crate::engine::sim::{
    PatientView, RoomView, World, DEFAULT_SIM_SPEED, MAX_SIM_SPEED, MIN_SIM_SPEED,
};
use crate::engine::stats::Metrics;
use crate::scenario::DEFAULT_PATIENT_COUNT;

pub struct AppState {
    pub scenario: Arc<Scenario>,
    pub graph: Arc<DependencyGraph>,
    pub data_dir: PathBuf,
    pub live: RwLock<LiveState>,
    pub persisted: RwLock<LearningSnapshot>,
}

pub struct LiveState {
    pub world: Option<World>,
    pub running: bool,
    pub speed: f64,
    pub last_persist: Instant,
}

impl AppState {
    pub fn new(scenario: Scenario, graph: DependencyGraph, data_dir: PathBuf) -> Self {
        let snapshot = Learner::load_snapshot(&data_dir).unwrap_or_default();
        Self {
            scenario: Arc::new(scenario),
            graph: Arc::new(graph),
            data_dir,
            live: RwLock::new(LiveState {
                world: None,
                running: false,
                speed: DEFAULT_SIM_SPEED,
                last_persist: Instant::now(),
            }),
            persisted: RwLock::new(snapshot),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub started: bool,
    pub running: bool,
    pub paused: bool,
    pub finished: bool,
    pub strategy: Option<Strategy>,
    pub speed: f64,
    pub seed: Option<u64>,
    pub sim_time_min: f64,
    pub metrics: Metrics,
}

#[derive(Debug, Serialize)]
pub struct HistoryEntry {
    pub patient_id: u32,
    pub patient_name: String,
    pub room_id: String,
    pub room_name: String,
    pub arrival_at_room: f64,
    pub service_start: f64,
    pub service_end: f64,
    pub wait_min: f64,
    pub service_min: f64,
}

#[derive(Debug, Serialize)]
pub struct StrategyResult {
    pub strategy: Strategy,
    pub label: String,
    pub metrics: Metrics,
    pub makespan_min: f64,
}

#[derive(Debug, Serialize)]
pub struct CompareResponse {
    pub seed: u64,
    pub patient_count: usize,
    pub results: Vec<StrategyResult>,
}

#[derive(Debug, Deserialize, Default)]
pub struct StartRequest {
    pub strategy: Option<Strategy>,
    pub speed: Option<f64>,
    pub seed: Option<u64>,
    pub patient_count: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub struct CompareRequest {
    pub seed: Option<u64>,
    pub patient_count: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct SpeedRequest {
    pub speed: f64,
}

pub fn api_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/config", get(get_config))
        .route("/sim/start", post(start_sim))
        .route("/sim/pause", post(pause_sim))
        .route("/sim/resume", post(resume_sim))
        .route("/sim/reset", post(reset_sim))
        .route("/sim/status", get(get_status))
        .route("/sim/speed", post(set_speed))
        .route("/sim/compare", post(compare_sim))
        .route("/rooms", get(get_rooms))
        .route("/patients", get(get_patients))
        .route("/history", get(get_history))
        .fallback(api_not_found)
        .with_state(state)
}

pub fn spawn_sim_driver(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_millis(100));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            drive_once(&state).await;
        }
    });
}

async fn drive_once(state: &Arc<AppState>) {
    let mut live = state.live.write().await;
    let speed = live.speed;
    let running = live.running;
    let save_due = live.last_persist.elapsed() >= Duration::from_secs(2);
    let Some(world) = live.world.as_mut() else {
        return;
    };
    let mut just_finished = false;
    if running && !world.is_finished() {
        world.advance_minutes(speed * 0.1);
        just_finished = world.is_finished();
    }
    let saved = if world.learner().is_dirty() && save_due {
        match world.persist(&state.data_dir) {
            Ok(()) => true,
            Err(error) => {
                eprintln!("warning: failed to persist learning data: {error}");
                false
            }
        }
    } else {
        false
    };
    if just_finished {
        live.running = false;
    }
    if saved {
        live.last_persist = Instant::now();
    }
}

fn build_status(live: &LiveState) -> StatusResponse {
    match &live.world {
        Some(world) => {
            let finished = world.is_finished();
            StatusResponse {
                started: true,
                running: live.running && !finished,
                paused: !live.running && !finished,
                finished,
                strategy: Some(world.strategy()),
                speed: live.speed,
                seed: Some(world.seed()),
                sim_time_min: world.now_min(),
                metrics: world.metrics(),
            }
        }
        None => StatusResponse {
            started: false,
            running: false,
            paused: false,
            finished: false,
            strategy: None,
            speed: live.speed,
            seed: None,
            sim_time_min: 0.0,
            metrics: Metrics::empty(),
        },
    }
}

fn persist_live_world(live: &mut LiveState, data_dir: &PathBuf) -> Option<LearningSnapshot> {
    let world = live.world.as_mut()?;
    let snapshot = world.learner().snapshot();
    if let Err(error) = world.persist(data_dir) {
        eprintln!("warning: failed to persist learning data: {error}");
    }
    Some(snapshot)
}

async fn get_config(State(state): State<Arc<AppState>>) -> Json<Scenario> {
    Json((*state.scenario).clone())
}

async fn start_sim(
    State(state): State<Arc<AppState>>,
    Json(request): Json<StartRequest>,
) -> Result<Json<StatusResponse>, ApiError> {
    let speed = clamp_speed(request.speed.unwrap_or(DEFAULT_SIM_SPEED))?;
    let strategy = request.strategy.unwrap_or(Strategy::Optimized);
    let seed = request.seed.unwrap_or_else(rand::random);
    let patient_count = request
        .patient_count
        .unwrap_or(DEFAULT_PATIENT_COUNT)
        .clamp(1, 200);

    let mut live = state.live.write().await;
    let snapshot = if let Some(snapshot) = persist_live_world(&mut live, &state.data_dir) {
        *state.persisted.write().await = snapshot.clone();
        snapshot
    } else {
        (*state.persisted.read().await).clone()
    };

    let learner = Learner::new(&state.scenario.rooms, Some(&snapshot));
    let world = World::new(
        &state.scenario,
        &state.graph,
        strategy,
        seed,
        patient_count,
        learner,
    );
    live.world = Some(world);
    live.running = true;
    live.speed = speed;
    live.last_persist = Instant::now();
    Ok(Json(build_status(&live)))
}

async fn pause_sim(State(state): State<Arc<AppState>>) -> Json<StatusResponse> {
    let mut live = state.live.write().await;
    if live.world.as_ref().is_some_and(|world| !world.is_finished()) {
        live.running = false;
        if let Some(snapshot) = persist_live_world(&mut live, &state.data_dir) {
            *state.persisted.write().await = snapshot;
        }
    }
    Json(build_status(&live))
}

async fn resume_sim(State(state): State<Arc<AppState>>) -> Json<StatusResponse> {
    let mut live = state.live.write().await;
    if live.world.as_ref().is_some_and(|world| !world.is_finished()) {
        live.running = true;
    }
    Json(build_status(&live))
}

async fn reset_sim(State(state): State<Arc<AppState>>) -> Json<StatusResponse> {
    let mut live = state.live.write().await;
    let snapshot = persist_live_world(&mut live, &state.data_dir);
    live.world = None;
    live.running = false;
    if let Some(snapshot) = snapshot {
        *state.persisted.write().await = snapshot;
    }
    Json(build_status(&live))
}

async fn set_speed(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SpeedRequest>,
) -> Result<Json<StatusResponse>, ApiError> {
    let speed = clamp_speed(request.speed)?;
    let mut live = state.live.write().await;
    live.speed = speed;
    Ok(Json(build_status(&live)))
}

async fn get_status(State(state): State<Arc<AppState>>) -> Json<StatusResponse> {
    let live = state.live.read().await;
    Json(build_status(&live))
}

async fn get_rooms(State(state): State<Arc<AppState>>) -> Json<Vec<RoomView>> {
    let live = state.live.read().await;
    let rooms = match &live.world {
        Some(world) => world.room_views(),
        None => {
            let persisted = state.persisted.read().await;
            let stats: HashMap<String, RoomStats> = persisted
                .room_stats
                .iter()
                .map(|stats| (stats.room_id.clone(), stats.clone()))
                .collect();
            RoomView::idle(&state.scenario.rooms, &stats)
        }
    };
    Json(rooms)
}

async fn get_patients(State(state): State<Arc<AppState>>) -> Json<Vec<PatientView>> {
    let live = state.live.read().await;
    Json(
        live.world
            .as_ref()
            .map(World::patient_views)
            .unwrap_or_default(),
    )
}

async fn get_history(State(state): State<Arc<AppState>>) -> Json<Vec<HistoryEntry>> {
    let live = state.live.read().await;
    let records = match &live.world {
        Some(world) => world.recent_visits(100),
        None => {
            let persisted = state.persisted.read().await;
            persisted
                .visit_log
                .iter()
                .rev()
                .take(100)
                .cloned()
                .collect::<Vec<VisitRecord>>()
        }
    };
    drop(live);
    let names = state.scenario.room_names();
    Json(
        records
            .into_iter()
            .map(|record| HistoryEntry {
                patient_id: record.patient_id,
                patient_name: record.patient_name,
                room_name: names
                    .get(&record.room_id)
                    .cloned()
                    .unwrap_or_else(|| record.room_id.clone()),
                room_id: record.room_id,
                arrival_at_room: record.arrival_at_room,
                service_start: record.service_start,
                service_end: record.service_end,
                wait_min: record.wait_min,
                service_min: record.service_min,
            })
            .collect(),
    )
}

async fn compare_sim(
    State(state): State<Arc<AppState>>,
    Json(request): Json<CompareRequest>,
) -> Result<Json<CompareResponse>, ApiError> {
    let (live_seed, live_count, live_stats) = {
        let live = state.live.read().await;
        match &live.world {
            Some(world) => (
                Some(world.seed()),
                Some(world.total_patients()),
                Some(world.learner().stats().clone()),
            ),
            None => (None, None, None),
        }
    };

    let stats = match live_stats {
        Some(stats) => stats,
        None => {
            let persisted = state.persisted.read().await;
            Learner::new(&state.scenario.rooms, Some(&persisted))
                .stats()
                .clone()
        }
    };
    let seed = request.seed.or(live_seed).unwrap_or_else(rand::random);
    let patient_count = request
        .patient_count
        .unwrap_or(live_count.unwrap_or(DEFAULT_PATIENT_COUNT))
        .clamp(1, 200);

    let mut results = Vec::with_capacity(3);
    for strategy in Strategy::all() {
        let learner = Learner::from_stats(&state.scenario.rooms, &stats);
        let mut world = World::new(
            &state.scenario,
            &state.graph,
            strategy,
            seed,
            patient_count,
            learner,
        );
        world.run_to_completion();
        results.push(StrategyResult {
            strategy,
            label: strategy.label().to_string(),
            metrics: world.metrics(),
            makespan_min: world.now_min(),
        });
    }

    Ok(Json(CompareResponse {
        seed,
        patient_count,
        results,
    }))
}

fn clamp_speed(speed: f64) -> Result<f64, ApiError> {
    if !speed.is_finite() {
        return Err(ApiError::bad_request("speed must be a finite number"));
    }
    Ok(speed.clamp(MIN_SIM_SPEED, MAX_SIM_SPEED))
}

async fn api_not_found() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": "not found" })),
    )
}

pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({ "error": self.message })),
        )
            .into_response()
    }
}
