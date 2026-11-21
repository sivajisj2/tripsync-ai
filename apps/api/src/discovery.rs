use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};
use moka::future::Cache;
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct DiscoveryState {
    pub client: reqwest::Client,
    pub key: String,
    pub cache: Cache<String, Vec<Place>>,
    pub limit: Arc<Semaphore>,
    pub metrics: DiscoveryMetrics,
}

#[derive(Clone, Default)]
pub struct DiscoveryMetrics {
    pub hits: Arc<AtomicU64>,
    pub misses: Arc<AtomicU64>,
    pub errors: Arc<AtomicU64>,
}

pub const CATEGORIES: [&str; 4] = ["tourist attractions", "restaurants", "hotels", "cafes"];

impl DiscoveryState {
    pub fn new(key: String) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(12))
                .build()
                .expect("HTTP client"),
            key,
            cache: Cache::builder()
                .time_to_live(Duration::from_secs(900))
                .max_capacity(100)
                .build(),
            limit: Arc::new(Semaphore::new(5)),
            metrics: DiscoveryMetrics::default(),
        }
    }
}

#[derive(Deserialize)]
pub struct SearchQuery {
    pub city: String,
    pub category: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Place {
    pub id: String,
    pub name: String,
    pub address: Option<String>,
    pub rating: Option<f64>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub maps_url: Option<String>,
}

#[derive(Deserialize)]
struct SerpResponse {
    #[serde(default)]
    local_results: Vec<SerpPlace>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct SerpPlace {
    title: String,
    place_id: Option<String>,
    data_id: Option<String>,
    address: Option<String>,
    rating: Option<f64>,
    gps_coordinates: Option<Coordinates>,
    link: Option<String>,
}

#[derive(Deserialize)]
struct Coordinates {
    latitude: f64,
    longitude: f64,
}

fn normalize(results: Vec<SerpPlace>) -> Vec<Place> {
    results
        .into_iter()
        .map(|p| {
            let coords = p.gps_coordinates;
            let id = p.place_id.or(p.data_id).unwrap_or_else(|| p.title.clone());

            Place {
                id,
                name: p.title,
                address: p.address,
                rating: p.rating,
                latitude: coords.as_ref().map(|c| c.latitude),
                longitude: coords.as_ref().map(|c| c.longitude),
                maps_url: p.link,
            }
        })
        .collect()
}

pub async fn search(
    State(state): State<DiscoveryState>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<Place>>, (StatusCode, String)> {
    let city = query.city.trim();

    if city.is_empty() || city.len() > 80 {
        return Err((
            StatusCode::BAD_REQUEST,
            "City must contain 1–80 characters".into(),
        ));
    }

    let category = query.category.as_deref().unwrap_or(CATEGORIES[0]).trim();

    if !CATEGORIES.contains(&category) {
        return Err((StatusCode::BAD_REQUEST, "Unsupported category".into()));
    }

    let places = search_places(&state, city, category).await?;

    Ok(Json(places))
}

pub async fn search_places(
    state: &DiscoveryState,
    city: &str,
    category: &str,
) -> Result<Vec<Place>, (StatusCode, String)> {
    let city = city.trim();
    if city.is_empty() || city.len() > 80 {
        return Err((
            StatusCode::BAD_REQUEST,
            "City must contain 1–80 characters".into(),
        ));
    }
    if !CATEGORIES.contains(&category) {
        return Err((StatusCode::BAD_REQUEST, "Unsupported category".into()));
    }
    let cache_key = format!("{}:{}", city.to_lowercase(), category);

    if let Some(cached) = state.cache.get(&cache_key).await {
        state.metrics.hits.fetch_add(1, Ordering::Relaxed);
        return Ok(cached);
    }

    state.metrics.misses.fetch_add(1, Ordering::Relaxed);
    let places = state
        .cache
        .try_get_with(cache_key, async {
            let _permit = state.limit.acquire().await.map_err(|e| e.to_string())?;

            let response = state
                .client
                .get("https://serpapi.com/search.json")
                .query(&[
                    ("engine", "google_maps"),
                    ("q", &format!("{category} in {city}")),
                    ("hl", "en"),
                    ("api_key", &state.key),
                ])
                .send()
                .await
                .map_err(|e| e.to_string())?;

            if !response.status().is_success() {
                return Err(format!("SerpApi HTTP {}", response.status()));
            }

            let payload: SerpResponse = response.json().await.map_err(|e| e.to_string())?;

            if let Some(error) = payload.error {
                return Err(error);
            }

            Ok::<Vec<Place>, String>(normalize(payload.local_results))
        })
        .await
        .map_err(|e| {
            state.metrics.errors.fetch_add(1, Ordering::Relaxed);
            tracing::error!("Discovery error: {e}");
            (
                StatusCode::BAD_GATEWAY,
                "Travel discovery temporarily unavailable".into(),
            )
        })?;

    Ok(places)
}
