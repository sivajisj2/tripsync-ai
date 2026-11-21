use axum::{
    Json, Router,
    http::{Method, StatusCode, header},
    routing::get,
};
use serde::Serialize;
use std::{net::SocketAddr, time::Instant};
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing::info;

#[derive(Serialize)]
struct HealthResponse {
    service: &'static str,
    status: &'static str,
    version: &'static str,
}

#[derive(Serialize)]
struct ApiResponse {
    message: &'static str,
    backend: &'static str,
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        service: "tripsync-api",
        status: "healthy",
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn root() -> Json<ApiResponse> {
    Json(ApiResponse {
        message: "TripSync AI API is running",
        backend: "Rust + Axum + Tokio",
    })
}

async fn concurrency_demo() -> Json<serde_json::Value> {
    let start = Instant::now();

    let tasks = (0..100).map(|_| {
        tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            1usize
        })
    });

    let results = futures_util::future::join_all(tasks).await;

    let completed = results.into_iter().filter_map(Result::ok).sum::<usize>();

    Json(serde_json::json!({
        "tasks_requested": 100,
        "tasks_completed": completed,
        "elapsed_ms": start.elapsed().as_millis(),
        "runtime": "Tokio"
    }))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cors = CorsLayer::new()
        .allow_origin("http://localhost:3000".parse::<axum::http::HeaderValue>()?)
        .allow_methods([Method::GET])
        .allow_headers([header::CONTENT_TYPE]);

    let app = Router::new()
        .route("/", get(root))
        .route("/health", get(health))
        .route("/api/demo/concurrency", get(concurrency_demo))
        .fallback(|| async { (StatusCode::NOT_FOUND, "Route not found") })
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    let addr = SocketAddr::from(([127, 0, 0, 1], 8080));
    let listener = tokio::net::TcpListener::bind(addr).await?;

    info!("TripSync API listening on http://{addr}");

    axum::serve(listener, app).await?;

    Ok(())
}
