use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use axum::{
    Json,
    body::Body,
    extract::State,
    http::{HeaderValue, Request},
    middleware::Next,
    response::Response,
};
use serde::Serialize;
use uuid::Uuid;

#[derive(Default)]
pub struct ApiMetrics {
    pub requests: AtomicU64,
    pub errors: AtomicU64,
    pub latency_micros: AtomicU64,
    pub recommendations: AtomicU64,
    pub recommendation_micros: AtomicU64,
}

#[derive(Clone)]
pub struct MetricsState {
    pub api: Arc<ApiMetrics>,
    pub discovery: crate::discovery::DiscoveryMetrics,
}

#[derive(Serialize)]
pub struct MetricsSnapshot {
    request_count: u64,
    error_count: u64,
    average_request_latency_ms: f64,
    serpapi_cache_hits: u64,
    serpapi_cache_misses: u64,
    serpapi_errors: u64,
    recommendation_count: u64,
    average_recommendation_latency_ms: f64,
}

pub async fn observe(
    State(metrics): State<Arc<ApiMetrics>>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let request_id = Uuid::new_v4().to_string();
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let started = std::time::Instant::now();
    if let Ok(header) = HeaderValue::from_str(&request_id) {
        request.headers_mut().insert("x-request-id", header);
    }
    let mut response = next.run(request).await;
    let elapsed = started.elapsed().as_micros().min(u64::MAX as u128) as u64;
    let status = response.status();

    metrics.requests.fetch_add(1, Ordering::Relaxed);
    metrics.latency_micros.fetch_add(elapsed, Ordering::Relaxed);
    if status.is_client_error() || status.is_server_error() {
        metrics.errors.fetch_add(1, Ordering::Relaxed);
    }
    if let Ok(header) = HeaderValue::from_str(&request_id) {
        response.headers_mut().insert("x-request-id", header);
    }

    tracing::info!(
        %request_id,
        %method,
        %path,
        %status,
        latency_ms = elapsed as f64 / 1000.0,
        "request completed"
    );
    response
}

pub async fn snapshot(State(state): State<MetricsState>) -> Json<MetricsSnapshot> {
    let metrics = state.api;
    let discovery = state.discovery;
    let requests = metrics.requests.load(Ordering::Relaxed);
    let recommendations = metrics.recommendations.load(Ordering::Relaxed);
    Json(MetricsSnapshot {
        request_count: requests,
        error_count: metrics.errors.load(Ordering::Relaxed),
        average_request_latency_ms: if requests == 0 {
            0.0
        } else {
            metrics.latency_micros.load(Ordering::Relaxed) as f64 / requests as f64 / 1000.0
        },
        serpapi_cache_hits: discovery.hits.load(Ordering::Relaxed),
        serpapi_cache_misses: discovery.misses.load(Ordering::Relaxed),
        serpapi_errors: discovery.errors.load(Ordering::Relaxed),
        recommendation_count: recommendations,
        average_recommendation_latency_ms: if recommendations == 0 {
            0.0
        } else {
            metrics.recommendation_micros.load(Ordering::Relaxed) as f64
                / recommendations as f64
                / 1000.0
        },
    })
}
