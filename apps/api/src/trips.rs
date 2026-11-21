use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use uuid::Uuid;

#[derive(Clone)]
pub struct TripState {
    pub db: SqlitePool,
}

#[derive(Serialize, FromRow)]
pub struct Trip {
    id: String,
    name: String,
    destination: String,
    created_at: String,
}

#[derive(Deserialize)]
pub struct CreateTrip {
    name: String,
    destination: String,
}

#[derive(Deserialize)]
pub struct AddStop {
    place_id: String,
    name: String,
    address: Option<String>,
}

#[derive(Serialize, FromRow)]
pub struct Stop {
    id: String,
    trip_id: String,
    place_id: String,
    name: String,
    address: Option<String>,
    votes: i64,
}

#[derive(Deserialize)]
pub struct VoteRequest {
    stop_id: String,
    voter_id: String,
}

type ApiError = (StatusCode, &'static str);

fn valid(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max
}

async fn create_trip(
    State(state): State<TripState>,
    Json(input): Json<CreateTrip>,
) -> Result<(StatusCode, Json<Trip>), ApiError> {
    if !valid(&input.name, 100) || !valid(&input.destination, 100) {
        return Err((StatusCode::BAD_REQUEST, "Invalid trip details"));
    }

    let id = Uuid::new_v4().to_string();

    sqlx::query("INSERT INTO trips (id, name, destination) VALUES (?, ?, ?)")
        .bind(&id)
        .bind(input.name.trim())
        .bind(input.destination.trim())
        .execute(&state.db)
        .await
        .map_err(internal_error)?;

    let trip = fetch_trip(&state.db, &id).await?;

    Ok((StatusCode::CREATED, Json(trip)))
}

async fn fetch_trip(db: &SqlitePool, id: &str) -> Result<Trip, ApiError> {
    sqlx::query_as::<_, Trip>(
        "SELECT id, name, destination, created_at
         FROM trips WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(db)
    .await
    .map_err(internal_error)?
    .ok_or((StatusCode::NOT_FOUND, "Trip not found"))
}

async fn get_trip(
    State(state): State<TripState>,
    Path(id): Path<String>,
) -> Result<Json<Trip>, ApiError> {
    Ok(Json(fetch_trip(&state.db, &id).await?))
}

async fn add_stop(
    State(state): State<TripState>,
    Path(trip_id): Path<String>,
    Json(input): Json<AddStop>,
) -> Result<(StatusCode, Json<Stop>), ApiError> {
    if !valid(&input.place_id, 255)
        || !valid(&input.name, 200)
        || input.address.as_ref().is_some_and(|a| a.len() > 500)
    {
        return Err((StatusCode::BAD_REQUEST, "Invalid place details"));
    }

    fetch_trip(&state.db, &trip_id).await?;

    let id = Uuid::new_v4().to_string();

    let result = sqlx::query(
        "INSERT INTO trip_stops
         (id, trip_id, place_id, name, address)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&trip_id)
    .bind(&input.place_id)
    .bind(&input.name)
    .bind(&input.address)
    .execute(&state.db)
    .await;

    match result {
        Ok(_) => {}
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            return Err((StatusCode::CONFLICT, "Place already added"));
        }
        Err(e) => return Err(internal_error(e)),
    }

    Ok((
        StatusCode::CREATED,
        Json(Stop {
            id,
            trip_id,
            place_id: input.place_id,
            name: input.name,
            address: input.address,
            votes: 0,
        }),
    ))
}

async fn list_stops(
    State(state): State<TripState>,
    Path(trip_id): Path<String>,
) -> Result<Json<Vec<Stop>>, ApiError> {
    fetch_trip(&state.db, &trip_id).await?;

    let stops = sqlx::query_as::<_, Stop>(
        "SELECT s.id, s.trip_id, s.place_id,
                s.name, s.address,
                COUNT(v.voter_id) AS votes
         FROM trip_stops s
         LEFT JOIN votes v ON v.stop_id = s.id
         WHERE s.trip_id = ?
         GROUP BY s.id
         ORDER BY votes DESC, s.added_at ASC",
    )
    .bind(trip_id)
    .fetch_all(&state.db)
    .await
    .map_err(internal_error)?;

    Ok(Json(stops))
}

async fn vote(
    State(state): State<TripState>,
    Path(trip_id): Path<String>,
    Json(input): Json<VoteRequest>,
) -> Result<StatusCode, ApiError> {
    if Uuid::parse_str(&input.voter_id).is_err() || Uuid::parse_str(&input.stop_id).is_err() {
        return Err((StatusCode::BAD_REQUEST, "Invalid vote identifiers"));
    }

    let result = sqlx::query(
        "INSERT INTO votes (trip_id, stop_id, voter_id)
         SELECT ?, id, ?
         FROM trip_stops
         WHERE id = ? AND trip_id = ?",
    )
    .bind(&trip_id)
    .bind(&input.voter_id)
    .bind(&input.stop_id)
    .bind(&trip_id)
    .execute(&state.db)
    .await;

    match result {
        Ok(r) if r.rows_affected() == 1 => Ok(StatusCode::CREATED),
        Ok(_) => Err((StatusCode::NOT_FOUND, "Stop not found")),
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            Err((StatusCode::CONFLICT, "Already voted"))
        }
        Err(e) => Err(internal_error(e)),
    }
}

fn internal_error(error: sqlx::Error) -> ApiError {
    tracing::error!(%error, "Database operation failed");
    (StatusCode::INTERNAL_SERVER_ERROR, "Database error")
}

pub fn router(state: TripState) -> Router {
    Router::new()
        .route("/api/trips", post(create_trip))
        .route("/api/trips/{id}", get(get_trip))
        .route("/api/trips/{id}/stops", post(add_stop).get(list_stops))
        .route("/api/trips/{id}/votes", post(vote))
        .with_state(state)
}
