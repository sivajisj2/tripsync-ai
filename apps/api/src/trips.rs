use crate::realtime::RealtimeState;
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
    pub realtime: RealtimeState,
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
    latitude: Option<f64>,
    longitude: Option<f64>,
    rating: Option<f64>,
    category: Option<String>,
}

#[derive(Serialize, FromRow)]
pub struct Stop {
    id: String,
    trip_id: String,
    place_id: String,
    name: String,
    address: Option<String>,
    votes: i64,
    latitude: Option<f64>,
    longitude: Option<f64>,
    rating: Option<f64>,
    category: Option<String>,
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
        || input.category.as_ref().is_some_and(|c| c.len() > 80)
        || input
            .rating
            .is_some_and(|rating| !(0.0..=5.0).contains(&rating))
        || input
            .latitude
            .is_some_and(|latitude| !(-90.0..=90.0).contains(&latitude))
        || input
            .longitude
            .is_some_and(|longitude| !(-180.0..=180.0).contains(&longitude))
    {
        return Err((StatusCode::BAD_REQUEST, "Invalid place details"));
    }

    fetch_trip(&state.db, &trip_id).await?;

    let id = Uuid::new_v4().to_string();

    let result = sqlx::query(
        "INSERT INTO trip_stops
         (id, trip_id, place_id, name, address, latitude, longitude, rating, category)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&trip_id)
    .bind(&input.place_id)
    .bind(&input.name)
    .bind(&input.address)
    .bind(input.latitude)
    .bind(input.longitude)
    .bind(input.rating)
    .bind(&input.category)
    .execute(&state.db)
    .await;
    match result {
        Ok(_) => {}
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            return Err((StatusCode::CONFLICT, "Place already added"));
        }
        Err(e) => return Err(internal_error(e)),
    }

    state.realtime.publish(&trip_id, "stop_added").await;

    Ok((
        StatusCode::CREATED,
        Json(Stop {
            id,
            trip_id,
            place_id: input.place_id,
            name: input.name,
            address: input.address,
            votes: 0,
            latitude: input.latitude,
            longitude: input.longitude,
            rating: input.rating,
            category: input.category,
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
                , s.latitude, s.longitude, s.rating, s.category
         FROM trip_stops s
         LEFT JOIN votes v ON v.stop_id = s.id
         WHERE s.trip_id = ?
         GROUP BY s.id
         ORDER BY COALESCE(s.itinerary_position, 2147483647), votes DESC, s.added_at ASC",
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
        Ok(r) if r.rows_affected() == 1 => {
            state.realtime.publish(&trip_id, "vote_added").await;
            Ok(StatusCode::CREATED)
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::realtime::RealtimeState;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn test_state() -> TripState {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&db).await.unwrap();
        TripState {
            realtime: RealtimeState::new(db.clone()),
            db,
        }
    }

    #[tokio::test]
    async fn duplicate_vote_returns_conflict() {
        let state = test_state().await;
        let trip_id = Uuid::new_v4().to_string();
        let stop_id = Uuid::new_v4().to_string();
        let voter_id = Uuid::new_v4().to_string();

        sqlx::query("INSERT INTO trips (id, name, destination) VALUES (?, ?, ?)")
            .bind(&trip_id)
            .bind("Test trip")
            .bind("Test destination")
            .execute(&state.db)
            .await
            .unwrap();
        sqlx::query("INSERT INTO trip_stops (id, trip_id, place_id, name) VALUES (?, ?, ?, ?)")
            .bind(&stop_id)
            .bind(&trip_id)
            .bind("place-1")
            .bind("Test place")
            .execute(&state.db)
            .await
            .unwrap();

        let input = VoteRequest {
            stop_id: stop_id.clone(),
            voter_id: voter_id.clone(),
        };
        assert_eq!(
            vote(State(state.clone()), Path(trip_id.clone()), Json(input),)
                .await
                .unwrap(),
            StatusCode::CREATED
        );

        let duplicate = vote(
            State(state),
            Path(trip_id),
            Json(VoteRequest { stop_id, voter_id }),
        )
        .await
        .unwrap_err();
        assert_eq!(duplicate.0, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn concurrent_unique_votes_are_counted_once_and_duplicate_voters_conflict() {
        use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode};
        use std::{str::FromStr, time::Duration};

        let file = std::env::temp_dir().join(format!("tripsync-votes-{}.db", Uuid::new_v4()));
        let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", file.display()))
            .unwrap()
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(10));
        let db = SqlitePoolOptions::new()
            .max_connections(8)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&db).await.unwrap();
        let state = TripState {
            realtime: RealtimeState::new(db.clone()),
            db: db.clone(),
        };
        let trip_id = Uuid::new_v4().to_string();
        let stop_id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO trips (id, name, destination) VALUES (?, ?, ?)")
            .bind(&trip_id)
            .bind("Concurrent test")
            .bind("Test city")
            .execute(&db)
            .await
            .unwrap();
        sqlx::query("INSERT INTO trip_stops (id, trip_id, place_id, name) VALUES (?, ?, ?, ?)")
            .bind(&stop_id)
            .bind(&trip_id)
            .bind("place")
            .bind("Place")
            .execute(&db)
            .await
            .unwrap();

        let writes = (0..100).map(|_| {
            let state = state.clone();
            let trip_id = trip_id.clone();
            let stop_id = stop_id.clone();
            tokio::spawn(async move {
                vote(
                    State(state),
                    Path(trip_id),
                    Json(VoteRequest {
                        stop_id,
                        voter_id: Uuid::new_v4().to_string(),
                    }),
                )
                .await
            })
        });
        let results = futures_util::future::join_all(writes).await;
        assert!(
            results
                .into_iter()
                .all(|result| result.unwrap() == Ok(StatusCode::CREATED))
        );

        let voter_id = Uuid::new_v4().to_string();
        let duplicate_writes = (0..10).map(|_| {
            let state = state.clone();
            let trip_id = trip_id.clone();
            let stop_id = stop_id.clone();
            let voter_id = voter_id.clone();
            tokio::spawn(async move {
                vote(
                    State(state),
                    Path(trip_id),
                    Json(VoteRequest { stop_id, voter_id }),
                )
                .await
            })
        });
        let duplicates = futures_util::future::join_all(duplicate_writes).await;
        let created = duplicates
            .iter()
            .filter(|result| matches!(result.as_ref().unwrap(), Ok(StatusCode::CREATED)))
            .count();
        let conflicts = duplicates
            .iter()
            .filter(|result| matches!(result.as_ref().unwrap(), Err((StatusCode::CONFLICT, _))))
            .count();
        assert_eq!((created, conflicts), (1, 9));

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM votes WHERE stop_id = ?")
            .bind(&stop_id)
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(count, 101);
        db.close().await;
        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
    }
}
