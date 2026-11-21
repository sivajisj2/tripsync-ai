use std::{cmp::Ordering, collections::HashMap, sync::Arc, time::Instant};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use uuid::Uuid;

use crate::{
    discovery::{self, DiscoveryState, Place},
    metrics::ApiMetrics,
    realtime::RealtimeState,
};

const RATING_WEIGHT: f64 = 0.30;
const PREFERENCE_WEIGHT: f64 = 0.30;
const VOTE_WEIGHT: f64 = 0.25;
const LOCATION_WEIGHT: f64 = 0.15;

#[derive(Clone)]
pub struct SmartState {
    pub db: SqlitePool,
    pub discovery: DiscoveryState,
    pub realtime: RealtimeState,
    pub metrics: Arc<ApiMetrics>,
}

type ApiError = (StatusCode, String);

#[derive(Deserialize)]
pub struct RecommendationQuery {
    pub city: Option<String>,
}

#[derive(Serialize)]
pub struct Recommendation {
    #[serde(flatten)]
    place: Place,
    score: f64,
    explanation: Vec<String>,
    votes: i64,
    category: String,
    in_itinerary: bool,
}

#[derive(Deserialize, Serialize, FromRow, Clone)]
pub struct Preference {
    category: String,
    weight: i64,
}

#[derive(Deserialize)]
pub struct PreferenceRequest {
    preferences: Vec<Preference>,
}

#[derive(Serialize, FromRow, Clone)]
pub struct PlanItem {
    stop_id: String,
    name: String,
    address: Option<String>,
    latitude: Option<f64>,
    longitude: Option<f64>,
    rating: Option<f64>,
    category: Option<String>,
    votes: i64,
    position: i64,
    score: f64,
    explanation: String,
}

#[derive(Serialize)]
pub struct SmartPlan {
    id: String,
    created_at: String,
    items: Vec<PlanItem>,
}

#[derive(Deserialize)]
pub struct ReorderPlanRequest {
    stop_ids: Vec<String>,
}

#[derive(FromRow)]
struct PlanningStop {
    stop_id: String,
    name: String,
    latitude: Option<f64>,
    longitude: Option<f64>,
    rating: Option<f64>,
    category: Option<String>,
    votes: i64,
}

async fn ensure_trip(db: &SqlitePool, trip_id: &str) -> Result<(), ApiError> {
    let exists = sqlx::query_scalar::<_, i64>("SELECT EXISTS(SELECT 1 FROM trips WHERE id = ?)")
        .bind(trip_id)
        .fetch_one(db)
        .await
        .map_err(internal)?;
    if exists == 0 {
        Err((StatusCode::NOT_FOUND, "Trip not found".into()))
    } else {
        Ok(())
    }
}

fn score_place(place: &Place, category: &str, preference: f64, votes: i64) -> (f64, Vec<String>) {
    let rating_signal = place
        .rating
        .map(|rating| (rating / 5.0).clamp(0.0, 1.0))
        .unwrap_or(0.5);
    let vote_signal = votes.clamp(0, 10) as f64 / 10.0;
    let has_location = place.latitude.is_some() && place.longitude.is_some();
    let location_signal = if has_location { 1.0 } else { 0.5 };
    let score = 100.0
        * (RATING_WEIGHT * rating_signal
            + PREFERENCE_WEIGHT * preference
            + VOTE_WEIGHT * vote_signal
            + LOCATION_WEIGHT * location_signal);

    let mut explanation = Vec::with_capacity(4);
    if let Some(rating) = place.rating {
        explanation.push(format!("Rated {rating:.1} out of 5"));
    } else {
        explanation.push("No rating available; neutral rating weight used".into());
    }
    if votes > 0 {
        explanation.push(format!(
            "{votes} group {}",
            if votes == 1 { "vote" } else { "votes" }
        ));
    } else {
        explanation.push("No group votes yet".into());
    }
    explanation.push(format!(
        "Group preference for {category}: {:.0} out of 5",
        preference * 5.0
    ));
    explanation.push(if has_location {
        "Coordinates available for route ordering".into()
    } else {
        "Location coordinates unavailable".into()
    });
    (score, explanation)
}

async fn recommendations(
    State(state): State<SmartState>,
    Path(trip_id): Path<String>,
    Query(query): Query<RecommendationQuery>,
) -> Result<Json<Vec<Recommendation>>, ApiError> {
    let started = Instant::now();
    ensure_trip(&state.db, &trip_id).await?;
    let destination = match query.city {
        Some(city) if !city.trim().is_empty() => city.trim().to_owned(),
        _ => sqlx::query_scalar::<_, String>("SELECT destination FROM trips WHERE id = ?")
            .bind(&trip_id)
            .fetch_one(&state.db)
            .await
            .map_err(internal)?,
    };
    if destination.len() > 80 {
        return Err((
            StatusCode::BAD_REQUEST,
            "City must contain at most 80 characters".into(),
        ));
    }

    let preferences = sqlx::query_as::<_, Preference>(
        "SELECT category, weight FROM trip_preferences WHERE trip_id = ?",
    )
    .bind(&trip_id)
    .fetch_all(&state.db)
    .await
    .map_err(internal)?;
    let preference_map: HashMap<String, f64> = preferences
        .into_iter()
        .map(|item| (item.category, item.weight as f64 / 5.0))
        .collect();
    let votes = sqlx::query_as::<_, (String, i64)>(
        "SELECT s.place_id, COUNT(v.voter_id) FROM trip_stops s
         LEFT JOIN votes v ON v.stop_id = s.id
         WHERE s.trip_id = ? GROUP BY s.place_id",
    )
    .bind(&trip_id)
    .fetch_all(&state.db)
    .await
    .map_err(internal)?
    .into_iter()
    .collect::<HashMap<_, _>>();
    let saved =
        sqlx::query_scalar::<_, String>("SELECT place_id FROM trip_stops WHERE trip_id = ?")
            .bind(&trip_id)
            .fetch_all(&state.db)
            .await
            .map_err(internal)?
            .into_iter()
            .collect::<std::collections::HashSet<_>>();

    let results =
        futures_util::future::join_all(discovery::CATEGORIES.into_iter().map(|category| async {
            let found = discovery::search_places(&state.discovery, &destination, category).await?;
            Ok::<_, ApiError>(
                found
                    .into_iter()
                    .map(|place| (place, category.to_owned()))
                    .collect::<Vec<_>>(),
            )
        }))
        .await;
    let mut candidates = Vec::new();
    for result in results {
        candidates.extend(result?);
    }

    let mut unique = HashMap::<String, (Place, String)>::new();
    for (place, category) in candidates {
        unique
            .entry(place.id.clone())
            .and_modify(|current| {
                let candidate_quality = (
                    place.rating.is_some(),
                    place.latitude.is_some() && place.longitude.is_some(),
                );
                let current_quality = (
                    current.0.rating.is_some(),
                    current.0.latitude.is_some() && current.0.longitude.is_some(),
                );
                let candidate_preference = preference_map.get(&category).copied().unwrap_or(0.5);
                let current_preference = preference_map.get(&current.1).copied().unwrap_or(0.5);
                if candidate_preference > current_preference
                    || (candidate_preference == current_preference
                        && candidate_quality > current_quality)
                {
                    *current = (place.clone(), category.clone());
                }
            })
            .or_insert((place, category));
    }

    let mut ranked = unique
        .into_iter()
        .map(|(id, (place, category))| {
            let preference = preference_map.get(&category).copied().unwrap_or(0.5);
            let (score, explanation) = score_place(
                &place,
                &category,
                preference,
                votes.get(&id).copied().unwrap_or(0),
            );
            Recommendation {
                place,
                score: (score * 100.0).round() / 100.0,
                explanation,
                votes: votes.get(&id).copied().unwrap_or(0),
                category,
                in_itinerary: saved.contains(&id),
            }
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(Ordering::Equal)
            .then_with(|| {
                b.place
                    .rating
                    .partial_cmp(&a.place.rating)
                    .unwrap_or(Ordering::Equal)
            })
            .then_with(|| a.place.name.cmp(&b.place.name))
            .then_with(|| a.place.id.cmp(&b.place.id))
    });

    state
        .metrics
        .recommendations
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    state.metrics.recommendation_micros.fetch_add(
        started.elapsed().as_micros().min(u64::MAX as u128) as u64,
        std::sync::atomic::Ordering::Relaxed,
    );
    tracing::info!(
        trip_id,
        candidate_count = ranked.len(),
        elapsed_ms = started.elapsed().as_millis(),
        "recommendations ranked"
    );
    Ok(Json(ranked))
}

async fn get_preferences(
    State(state): State<SmartState>,
    Path(trip_id): Path<String>,
) -> Result<Json<Vec<Preference>>, ApiError> {
    ensure_trip(&state.db, &trip_id).await?;
    let preferences = sqlx::query_as::<_, Preference>(
        "SELECT category, weight FROM trip_preferences WHERE trip_id = ? ORDER BY category",
    )
    .bind(trip_id)
    .fetch_all(&state.db)
    .await
    .map_err(internal)?;
    Ok(Json(preferences))
}

async fn set_preferences(
    State(state): State<SmartState>,
    Path(trip_id): Path<String>,
    Json(input): Json<PreferenceRequest>,
) -> Result<Json<Vec<Preference>>, ApiError> {
    ensure_trip(&state.db, &trip_id).await?;
    let mut seen = std::collections::HashSet::new();
    if input.preferences.len() > discovery::CATEGORIES.len()
        || input.preferences.iter().any(|p| {
            !discovery::CATEGORIES.contains(&p.category.as_str())
                || !(0..=5).contains(&p.weight)
                || !seen.insert(p.category.as_str())
        })
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "Invalid category preferences".into(),
        ));
    }
    let mut tx = state.db.begin().await.map_err(internal)?;
    sqlx::query("DELETE FROM trip_preferences WHERE trip_id = ?")
        .bind(&trip_id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    for preference in input.preferences {
        sqlx::query("INSERT INTO trip_preferences (trip_id, category, weight) VALUES (?, ?, ?)")
            .bind(&trip_id)
            .bind(&preference.category)
            .bind(preference.weight)
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
    }
    tx.commit().await.map_err(internal)?;
    get_preferences(State(state), Path(trip_id)).await
}

async fn planning_stops(db: &SqlitePool, trip_id: &str) -> Result<Vec<PlanningStop>, ApiError> {
    sqlx::query_as::<_, PlanningStop>(
        "SELECT s.id AS stop_id, s.name, s.latitude, s.longitude, s.rating, s.category,
                COUNT(v.voter_id) AS votes
         FROM trip_stops s LEFT JOIN votes v ON v.stop_id = s.id
         WHERE s.trip_id = ? GROUP BY s.id ORDER BY s.added_at, s.id",
    )
    .bind(trip_id)
    .fetch_all(db)
    .await
    .map_err(internal)
}

fn plan_score(stop: &PlanningStop, preferences: &HashMap<String, f64>) -> (f64, String) {
    let rating = stop
        .rating
        .map(|rating| (rating / 5.0).clamp(0.0, 1.0))
        .unwrap_or(0.5);
    let pref = stop
        .category
        .as_ref()
        .and_then(|category| preferences.get(category))
        .copied()
        .unwrap_or(0.5);
    let votes = stop.votes.clamp(0, 10) as f64 / 10.0;
    let location = if stop.latitude.is_some() && stop.longitude.is_some() {
        1.0
    } else {
        0.5
    };
    let score = (100.0
        * (RATING_WEIGHT * rating
            + PREFERENCE_WEIGHT * pref
            + VOTE_WEIGHT * votes
            + LOCATION_WEIGHT * location)
        * 100.0)
        .round()
        / 100.0;
    let explanation = format!(
        "Rating signal {:.0}%, preference signal {:.0}%, {} group votes; location {}.",
        rating * 100.0,
        pref * 100.0,
        stop.votes,
        if location > 0.5 {
            "available"
        } else {
            "not available"
        }
    );
    (score, explanation)
}

fn order_plan(
    mut stops: Vec<PlanningStop>,
    preferences: &HashMap<String, f64>,
) -> Vec<(PlanningStop, f64, String)> {
    let mut scored = stops
        .drain(..)
        .map(|stop| {
            let (score, explanation) = plan_score(&stop, preferences);
            (stop, score, explanation)
        })
        .collect::<Vec<_>>();
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.0.name.cmp(&b.0.name))
            .then_with(|| a.0.stop_id.cmp(&b.0.stop_id))
    });
    let mut ordered = Vec::with_capacity(scored.len());
    let mut current: Option<(f64, f64)> = None;
    while !scored.is_empty() {
        let next = if let Some((lat, lon)) = current {
            let scored_candidate = scored
                .iter()
                .take(3)
                .enumerate()
                .filter_map(|(i, (stop, _, _))| {
                    Some((
                        i,
                        squared_distance(lat, lon, stop.latitude?, stop.longitude?),
                        stop.votes,
                    ))
                })
                .min_by(|a, b| {
                    a.1.partial_cmp(&b.1)
                        .unwrap_or(Ordering::Equal)
                        .then_with(|| b.2.cmp(&a.2))
                })
                .map(|(i, _, _)| i);
            scored_candidate.unwrap_or(0)
        } else {
            0
        };
        let item = scored.remove(next);
        if let (Some(lat), Some(lon)) = (item.0.latitude, item.0.longitude) {
            current = Some((lat, lon));
        }
        ordered.push(item);
    }
    ordered
}

fn squared_distance(lat_a: f64, lon_a: f64, lat_b: f64, lon_b: f64) -> f64 {
    (lat_a - lat_b).powi(2) + (lon_a - lon_b).powi(2)
}

async fn generate_plan(
    State(state): State<SmartState>,
    Path(trip_id): Path<String>,
) -> Result<(StatusCode, Json<SmartPlan>), ApiError> {
    ensure_trip(&state.db, &trip_id).await?;
    let preferences = sqlx::query_as::<_, Preference>(
        "SELECT category, weight FROM trip_preferences WHERE trip_id = ?",
    )
    .bind(&trip_id)
    .fetch_all(&state.db)
    .await
    .map_err(internal)?;
    let preferences = preferences
        .into_iter()
        .map(|p| (p.category, p.weight as f64 / 5.0))
        .collect::<HashMap<_, _>>();
    let ordered = order_plan(planning_stops(&state.db, &trip_id).await?, &preferences);
    let plan_id = Uuid::new_v4().to_string();
    let mut tx = state.db.begin().await.map_err(internal)?;
    sqlx::query("DELETE FROM itinerary_plans WHERE trip_id = ?")
        .bind(&trip_id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    sqlx::query("INSERT INTO itinerary_plans (id, trip_id) VALUES (?, ?)")
        .bind(&plan_id)
        .bind(&trip_id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    for (position, (stop, score, explanation)) in ordered.iter().enumerate() {
        sqlx::query("INSERT INTO itinerary_plan_stops (plan_id, stop_id, position, score, explanation) VALUES (?, ?, ?, ?, ?)")
            .bind(&plan_id).bind(&stop.stop_id).bind(position as i64).bind(score).bind(explanation).execute(&mut *tx).await.map_err(internal)?;
    }
    tx.commit().await.map_err(internal)?;
    let plan = fetch_plan(&state.db, &trip_id, &plan_id).await?;
    Ok((StatusCode::CREATED, Json(plan)))
}

async fn fetch_plan(db: &SqlitePool, trip_id: &str, plan_id: &str) -> Result<SmartPlan, ApiError> {
    let (id, created_at) = sqlx::query_as::<_, (String, String)>(
        "SELECT id, created_at FROM itinerary_plans WHERE id = ? AND trip_id = ?",
    )
    .bind(plan_id)
    .bind(trip_id)
    .fetch_optional(db)
    .await
    .map_err(internal)?
    .ok_or((StatusCode::NOT_FOUND, "Plan not found".into()))?;
    let items = sqlx::query_as::<_, PlanItem>(
        "SELECT p.stop_id, s.name, s.address, s.latitude, s.longitude, s.rating, s.category,
                (SELECT COUNT(*) FROM votes v WHERE v.stop_id = s.id) AS votes,
                p.position, p.score, p.explanation
         FROM itinerary_plan_stops p JOIN trip_stops s ON s.id = p.stop_id
         WHERE p.plan_id = ? ORDER BY p.position",
    )
    .bind(&id)
    .fetch_all(db)
    .await
    .map_err(internal)?;
    Ok(SmartPlan {
        id,
        created_at,
        items,
    })
}

async fn get_plan(
    State(state): State<SmartState>,
    Path(trip_id): Path<String>,
) -> Result<Json<Option<SmartPlan>>, ApiError> {
    ensure_trip(&state.db, &trip_id).await?;
    let id = sqlx::query_scalar::<_, String>(
        "SELECT id FROM itinerary_plans WHERE trip_id = ? ORDER BY created_at DESC LIMIT 1",
    )
    .bind(&trip_id)
    .fetch_optional(&state.db)
    .await
    .map_err(internal)?;
    match id {
        Some(id) => Ok(Json(Some(fetch_plan(&state.db, &trip_id, &id).await?))),
        None => Ok(Json(None)),
    }
}

async fn reorder_plan(
    State(state): State<SmartState>,
    Path((trip_id, plan_id)): Path<(String, String)>,
    Json(input): Json<ReorderPlanRequest>,
) -> Result<Json<SmartPlan>, ApiError> {
    let mut tx = state.db.begin().await.map_err(internal)?;
    let belongs = sqlx::query_scalar::<_, i64>(
        "SELECT EXISTS(SELECT 1 FROM itinerary_plans WHERE id = ? AND trip_id = ?)",
    )
    .bind(&plan_id)
    .bind(&trip_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal)?;
    if belongs == 0 {
        return Err((StatusCode::NOT_FOUND, "Plan not found".into()));
    }
    let existing = sqlx::query_scalar::<_, String>(
        "SELECT stop_id FROM itinerary_plan_stops WHERE plan_id = ? ORDER BY position",
    )
    .bind(&plan_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal)?;
    let mut expected = existing.clone();
    let mut provided = input.stop_ids.clone();
    expected.sort();
    provided.sort();
    if expected != provided {
        return Err((
            StatusCode::BAD_REQUEST,
            "Plan order must contain each stop exactly once".into(),
        ));
    }
    let existing_data = sqlx::query_as::<_, (String, f64, String)>(
        "SELECT stop_id, score, explanation FROM itinerary_plan_stops WHERE plan_id = ?",
    )
    .bind(&plan_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal)?
    .into_iter()
    .map(|(stop_id, score, explanation)| (stop_id, (score, explanation)))
    .collect::<HashMap<_, _>>();
    sqlx::query("DELETE FROM itinerary_plan_stops WHERE plan_id = ?")
        .bind(&plan_id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    for (position, stop_id) in input.stop_ids.iter().enumerate() {
        let Some((score, explanation)) = existing_data.get(stop_id) else {
            return Err((StatusCode::BAD_REQUEST, "Unknown stop in plan".into()));
        };
        sqlx::query("INSERT INTO itinerary_plan_stops (plan_id, stop_id, position, score, explanation) VALUES (?, ?, ?, ?, ?)")
            .bind(&plan_id).bind(stop_id).bind(position as i64).bind(score).bind(explanation).execute(&mut *tx).await.map_err(internal)?;
    }
    tx.commit().await.map_err(internal)?;
    Ok(Json(fetch_plan(&state.db, &trip_id, &plan_id).await?))
}

async fn apply_plan(
    State(state): State<SmartState>,
    Path((trip_id, plan_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let mut tx = state.db.begin().await.map_err(internal)?;
    let belongs = sqlx::query_scalar::<_, i64>(
        "SELECT EXISTS(SELECT 1 FROM itinerary_plans WHERE id = ? AND trip_id = ?)",
    )
    .bind(&plan_id)
    .bind(&trip_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal)?;
    if belongs == 0 {
        return Err((StatusCode::NOT_FOUND, "Plan not found".into()));
    }
    let rows = sqlx::query_as::<_, (String, i64)>(
        "SELECT stop_id, position FROM itinerary_plan_stops WHERE plan_id = ? ORDER BY position",
    )
    .bind(&plan_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal)?;
    sqlx::query("UPDATE trip_stops SET itinerary_position = NULL WHERE trip_id = ?")
        .bind(&trip_id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    let mut position = 0_i64;
    for (stop_id, _) in rows {
        let result = sqlx::query(
            "UPDATE trip_stops SET itinerary_position = ? WHERE id = ? AND trip_id = ?",
        )
        .bind(position)
        .bind(stop_id)
        .bind(&trip_id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
        if result.rows_affected() == 1 {
            position += 1;
        }
    }
    let extras = sqlx::query_scalar::<_, String>("SELECT id FROM trip_stops WHERE trip_id = ? AND itinerary_position IS NULL ORDER BY added_at, id")
        .bind(&trip_id).fetch_all(&mut *tx).await.map_err(internal)?;
    for stop_id in extras {
        sqlx::query("UPDATE trip_stops SET itinerary_position = ? WHERE id = ?")
            .bind(position)
            .bind(stop_id)
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
        position += 1;
    }
    tx.commit().await.map_err(internal)?;
    state.realtime.publish(&trip_id, "plan_applied").await;
    Ok(StatusCode::NO_CONTENT)
}

fn internal(error: sqlx::Error) -> ApiError {
    tracing::error!(%error, "Smart planning database operation failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "Database operation failed".into(),
    )
}

pub fn router(state: SmartState) -> Router {
    Router::new()
        .route("/api/trips/{id}/recommendations", get(recommendations))
        .route(
            "/api/trips/{id}/preferences",
            get(get_preferences).put(set_preferences),
        )
        .route(
            "/api/trips/{id}/smart-plan",
            get(get_plan).post(generate_plan),
        )
        .route(
            "/api/trips/{id}/smart-plan/{plan_id}",
            axum::routing::put(reorder_plan),
        )
        .route(
            "/api/trips/{id}/smart-plan/{plan_id}/apply",
            post(apply_plan),
        )
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(
        id: &str,
        rating: Option<f64>,
        latitude: Option<f64>,
        longitude: Option<f64>,
    ) -> Place {
        Place {
            id: id.into(),
            name: id.into(),
            address: None,
            rating,
            latitude,
            longitude,
            maps_url: None,
        }
    }

    #[test]
    fn ranking_is_deterministic_and_handles_missing_signals() {
        let place = place("unknown", None, None, None);
        let (score, explanation) = score_place(&place, "cafes", 0.5, 0);
        assert_eq!(score, 37.5);
        assert_eq!(explanation.len(), 4);
        assert!(explanation[0].contains("No rating"));
        assert!(explanation[1].contains("No group votes"));
        assert!(explanation[3].contains("unavailable"));
    }

    #[test]
    fn nearest_neighbor_uses_coordinates_and_stably_falls_back_without_them() {
        let make_stop = |id: &str, lat, lon| PlanningStop {
            stop_id: id.into(),
            name: id.into(),
            latitude: lat,
            longitude: lon,
            rating: Some(4.5),
            category: None,
            votes: 0,
        };
        let stops = vec![
            make_stop("a", Some(0.0), Some(0.0)),
            make_stop("b", Some(1.0), Some(1.0)),
            make_stop("c", None, None),
        ];
        let ordered = order_plan(stops, &HashMap::new());
        assert_eq!(
            ordered
                .iter()
                .map(|item| item.0.stop_id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
    }

    #[tokio::test]
    async fn generated_plan_is_persisted_and_only_applies_after_explicit_apply() {
        let db = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&db).await.unwrap();
        let trip_id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO trips (id, name, destination) VALUES (?, ?, ?)")
            .bind(&trip_id)
            .bind("Plan test")
            .bind("Test city")
            .execute(&db)
            .await
            .unwrap();
        let first = Uuid::new_v4().to_string();
        let second = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO trip_stops (id, trip_id, place_id, name, latitude, longitude, rating, category) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(&first).bind(&trip_id).bind("a").bind("A").bind(0.0).bind(0.0).bind(4.0).bind("cafes").execute(&db).await.unwrap();
        sqlx::query("INSERT INTO trip_stops (id, trip_id, place_id, name, latitude, longitude, rating, category) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(&second).bind(&trip_id).bind("b").bind("B").bind(1.0).bind(1.0).bind(5.0).bind("restaurants").execute(&db).await.unwrap();
        let realtime = RealtimeState::new(db.clone());
        let state = SmartState {
            db: db.clone(),
            discovery: DiscoveryState::new("unused".into()),
            realtime,
            metrics: Arc::new(ApiMetrics::default()),
        };

        let (StatusCode::CREATED, Json(plan)) =
            generate_plan(State(state.clone()), Path(trip_id.clone()))
                .await
                .unwrap()
        else {
            panic!("expected created plan")
        };
        assert_eq!(plan.items.len(), 2);
        let before_apply: Option<i64> =
            sqlx::query_scalar("SELECT itinerary_position FROM trip_stops WHERE id = ?")
                .bind(&first)
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(before_apply, None);
        let saved = get_plan(State(state.clone()), Path(trip_id.clone()))
            .await
            .unwrap()
            .0
            .unwrap();
        assert_eq!(saved.id, plan.id);
        apply_plan(State(state), Path((trip_id, plan.id)))
            .await
            .unwrap();
        let ordered = sqlx::query_as::<_, (String, Option<i64>)>(
            "SELECT id, itinerary_position FROM trip_stops ORDER BY itinerary_position",
        )
        .fetch_all(&db)
        .await
        .unwrap();
        assert_eq!(ordered.len(), 2);
        assert!(ordered.iter().all(|(_, position)| position.is_some()));
    }
}
