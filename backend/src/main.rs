mod api;
mod domain;
mod engine;
mod scenario;

use std::sync::Arc;

use axum::http::StatusCode;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

#[tokio::main]
async fn main() {
    let demo = scenario::demo_scenario();
    let graph = demo
        .validate()
        .expect("built-in demo scenario must be a valid DAG");
    let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let data_dir = std::env::var("MEDIROUTE_DATA_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| crate_dir.join("data"));
    if let Err(error) = std::fs::create_dir_all(&data_dir) {
        eprintln!("warning: could not create data directory: {error}");
    }

    let state = Arc::new(api::AppState::new(demo, graph, data_dir));
    api::spawn_sim_driver(state.clone());

    let dist = crate_dir
        .parent()
        .unwrap_or(crate_dir)
        .join("frontend")
        .join("dist");
    let index = dist.join("index.html");
    let base = axum::Router::new().nest("/api", api::api_router(state));
    let app = if index.exists() {
        base.fallback_service(ServeDir::new(&dist).not_found_service(ServeFile::new(index)))
    } else {
        base.fallback(|| async {
            (
                StatusCode::OK,
                "MediRoute API is running. Build the dashboard with: cd frontend && npm install && npm run build",
            )
        })
    }
    .layer(CorsLayer::permissive());

    let port = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8080);
    let address = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&address)
        .await
        .unwrap_or_else(|error| panic!("failed to bind {address}: {error}"));
    println!("MediRoute backend listening on http://{address}");
    axum::serve(listener, app)
        .await
        .expect("server failed");
}
