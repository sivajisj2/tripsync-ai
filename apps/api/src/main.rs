use axum::{
    Json, Router,
    extract::State,
    http::{Method, StatusCode, header},
    middleware,
    routing::get,
};
use serde::Serialize;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use std::str::FromStr;
use std::{net::SocketAddr, time::Instant};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::info;
mod auth;
mod discovery;

use discovery::DiscoveryState;

mod metrics;
mod nexconn;
mod realtime;
mod smart;
mod trips;
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

async fn readiness(
    State(db): axum::extract::State<sqlx::SqlitePool>,
) -> Result<Json<HealthResponse>, StatusCode> {
    sqlx::query_scalar::<_, i64>("SELECT 1")
        .fetch_one(&db)
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    Ok(Json(HealthResponse {
        service: "tripsync-api",
        status: "ready",
        version: env!("CARGO_PKG_VERSION"),
    }))
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

    dotenvy::dotenv().ok();

    let serpapi_key = std::env::var("SERPAPI_KEY").unwrap_or_default();

    let discovery_state = DiscoveryState::new(serpapi_key);

    let app_origin = std::env::var("APP_ORIGIN").unwrap_or_else(|_| "http://localhost:3000".into());
    let cors = CorsLayer::new()
        .allow_origin(app_origin.parse::<axum::http::HeaderValue>()?)
        .allow_credentials(true)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::OPTIONS])
        .allow_headers([
            header::CONTENT_TYPE,
            header::COOKIE,
            "x-csrf-token".parse()?,
        ]);

    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://tripsync.db".into());
    let db_options = SqliteConnectOptions::from_str(&database_url)?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(std::time::Duration::from_secs(5));

    let db = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(db_options)
        .await?;

    sqlx::migrate!("./migrations").run(&db).await?;

    let session_ttl = std::env::var("SESSION_TTL_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(auth::DEFAULT_SESSION_TTL)
        .clamp(300, 30 * 24 * 60 * 60);
    let secure_cookie = std::env::var("COOKIE_SECURE")
        .map(|value| value.eq_ignore_ascii_case("true"))
        .unwrap_or_else(|_| std::env::var("APP_ENV").is_ok_and(|value| value == "production"));
    let auth_state = auth::AuthState {
        db: db.clone(),
        app_origin: app_origin.clone(),
        secure_cookie,
        session_ttl,
        rate_limiter: auth::RateLimiter::default(),
    };

    let realtime_state = realtime::RealtimeState::new(db.clone());
    let nexconn_state = nexconn::NexconnState::new(db.clone(), app_origin.clone());
    let realtime_router = realtime::router(realtime_state.clone());
    let api_metrics = std::sync::Arc::new(metrics::ApiMetrics::default());
    let metrics_router = Router::new().route(
        "/metrics",
        get(metrics::snapshot).with_state(metrics::MetricsState {
            api: api_metrics.clone(),
            discovery: discovery_state.metrics.clone(),
        }),
    );
    let trip_router = trips::router(trips::TripState {
        db: db.clone(),
        realtime: realtime_state.clone(),
        app_origin: app_origin.clone(),
        nexconn: nexconn_state.clone(),
    });
    let smart_router = smart::router(smart::SmartState {
        db: db.clone(),
        discovery: discovery_state.clone(),
        realtime: realtime_state,
        metrics: api_metrics.clone(),
        app_origin: app_origin.clone(),
    });
    let nexconn_router = nexconn::router(nexconn_state);

    let app = Router::new()
        .route("/", get(root))
        .route("/health", get(health))
        .route("/ready", get(readiness).with_state(db.clone()))
        .route("/api/demo/concurrency", get(concurrency_demo))
        .route(
            "/api/places/search",
            get(discovery::search).with_state(discovery_state),
        )
        .merge(trip_router)
        .merge(auth::router(auth_state))
        .merge(nexconn_router)
        .merge(smart_router)
        .merge(realtime_router)
        .merge(metrics_router)
        .fallback(|| async { (StatusCode::NOT_FOUND, "Route not found") })
        .layer(cors)
        .layer(middleware::from_fn_with_state(
            api_metrics,
            metrics::observe,
        ))
        .layer(middleware::from_fn_with_state(db, auth::attach_session))
        .layer(TraceLayer::new_for_http());

    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8080);
    let addr: SocketAddr = format!("{host}:{port}").parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    info!("TripSync API listening on http://{addr}");

    axum::serve(listener, app).await?;

    Ok(())
}
