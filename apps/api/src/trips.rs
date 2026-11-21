use crate::{
    auth::{self, AuthenticatedUser, SessionMarker},
    nexconn::{self, NexconnState},
    realtime::RealtimeState,
};
use axum::{
    Json, Router,
    extract::{Extension, Path, State},
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
    pub app_origin: String,
    pub nexconn: NexconnState,
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

#[derive(Deserialize, Serialize, FromRow)]
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
}

type ApiError = (StatusCode, &'static str);

fn valid(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max
}

async fn create_trip(
    State(state): State<TripState>,
    user: AuthenticatedUser,
    headers: axum::http::HeaderMap,
    Json(input): Json<CreateTrip>,
) -> Result<(StatusCode, Json<Trip>), ApiError> {
    auth::verify_csrf(&headers, &user, &state.app_origin)
        .map_err(|_| (StatusCode::FORBIDDEN, "Invalid session or CSRF token"))?;
    if !valid(&input.name, 100) || !valid(&input.destination, 100) {
        return Err((StatusCode::BAD_REQUEST, "Invalid trip details"));
    }

    let id = Uuid::new_v4().to_string();

    let mut tx = state.db.begin().await.map_err(internal_error)?;
    sqlx::query("INSERT INTO trips (id, name, destination) VALUES (?, ?, ?)")
        .bind(&id)
        .bind(input.name.trim())
        .bind(input.destination.trim())
        .execute(&mut *tx)
        .await
        .map_err(internal_error)?;
    sqlx::query("INSERT INTO trip_members (trip_id, user_id, role) VALUES (?, ?, 'owner')")
        .bind(&id)
        .bind(&user.id)
        .execute(&mut *tx)
        .await
        .map_err(internal_error)?;
    tx.commit().await.map_err(internal_error)?;

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
    user: AuthenticatedUser,
    headers: axum::http::HeaderMap,
    Json(input): Json<AddStop>,
) -> Result<(StatusCode, Json<Stop>), ApiError> {
    auth::verify_csrf(&headers, &user, &state.app_origin)
        .map_err(|_| (StatusCode::FORBIDDEN, "Invalid session or CSRF token"))?;
    require_member(&state.db, &trip_id, &user.id).await?;
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
    user: AuthenticatedUser,
    headers: axum::http::HeaderMap,
    Json(input): Json<VoteRequest>,
) -> Result<StatusCode, ApiError> {
    auth::verify_csrf(&headers, &user, &state.app_origin)
        .map_err(|_| (StatusCode::FORBIDDEN, "Invalid session or CSRF token"))?;
    require_member(&state.db, &trip_id, &user.id).await?;
    if Uuid::parse_str(&input.stop_id).is_err() {
        return Err((StatusCode::BAD_REQUEST, "Invalid vote identifiers"));
    }

    let result = sqlx::query(
        "INSERT INTO votes (trip_id, stop_id, voter_id)
         SELECT ?, id, ?
         FROM trip_stops
         WHERE id = ? AND trip_id = ?",
    )
    .bind(&trip_id)
    .bind(&user.id)
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

pub async fn require_member(db: &SqlitePool, trip_id: &str, user_id: &str) -> Result<(), ApiError> {
    let is_member = sqlx::query_scalar::<_, i64>(
        "SELECT EXISTS(SELECT 1 FROM trip_members WHERE trip_id = ? AND user_id = ?)",
    )
    .bind(trip_id)
    .bind(user_id)
    .fetch_one(db)
    .await
    .map_err(internal_error)?;
    if is_member == 1 {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            "Join this trip before changing its itinerary",
        ))
    }
}

async fn join_trip(
    State(state): State<TripState>,
    Path(trip_id): Path<String>,
    user: AuthenticatedUser,
    headers: axum::http::HeaderMap,
) -> Result<StatusCode, ApiError> {
    auth::verify_csrf(&headers, &user, &state.app_origin)
        .map_err(|_| (StatusCode::FORBIDDEN, "Invalid session or CSRF token"))?;
    fetch_trip(&state.db, &trip_id).await?;
    sqlx::query("INSERT INTO trip_members (trip_id, user_id, role) VALUES (?, ?, 'member') ON CONFLICT(trip_id, user_id) DO NOTHING")
        .bind(&trip_id).bind(&user.id).execute(&state.db).await.map_err(internal_error)?;
    let joined = sqlx::query_scalar::<_, i64>(
        "SELECT EXISTS(SELECT 1 FROM nexconn_group_members WHERE trip_id = ? AND user_id = ?)",
    )
    .bind(&trip_id)
    .bind(&user.id)
    .fetch_one(&state.db)
    .await
    .map_err(internal_error)?;
    if joined == 0 {
        nexconn::sync_new_trip_member(&state.nexconn, &trip_id, &user)
            .await
            .map_err(|(status, _)| (status, "Could not synchronize trip chat membership"))?;
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn membership(
    State(state): State<TripState>,
    Path(trip_id): Path<String>,
    Extension(session): Extension<SessionMarker>,
) -> Result<Json<serde_json::Value>, ApiError> {
    fetch_trip(&state.db, &trip_id).await?;
    let member = if let Some(user) = session.0 {
        sqlx::query_scalar::<_, i64>(
            "SELECT EXISTS(SELECT 1 FROM trip_members WHERE trip_id = ? AND user_id = ?)",
        )
        .bind(trip_id)
        .bind(user.id)
        .fetch_one(&state.db)
        .await
        .map_err(internal_error)?
            == 1
    } else {
        false
    };
    Ok(Json(serde_json::json!({ "is_member": member })))
}

fn internal_error(error: sqlx::Error) -> ApiError {
    tracing::error!(%error, "Database operation failed");
    (StatusCode::INTERNAL_SERVER_ERROR, "Database error")
}

pub fn router(state: TripState) -> Router {
    Router::new()
        .route("/api/trips", post(create_trip))
        .route("/api/trips/{id}", get(get_trip))
        .route("/api/trips/{id}/membership", get(membership))
        .route("/api/trips/{id}/join", post(join_trip))
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
            nexconn: NexconnState::new(db.clone(), "http://localhost:3000".into()),
            db,
            app_origin: "http://localhost:3000".into(),
        }
    }

    fn authenticated_user(id: String) -> AuthenticatedUser {
        AuthenticatedUser {
            id,
            email: "member@example.com".into(),
            display_name: "Member".into(),
            csrf_token: "test-csrf".into(),
            session_hash: "unused".into(),
        }
    }

    fn csrf_headers() -> axum::http::HeaderMap {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::ORIGIN,
            axum::http::HeaderValue::from_static("http://localhost:3000"),
        );
        headers.insert(
            "x-csrf-token",
            axum::http::HeaderValue::from_static("test-csrf"),
        );
        headers
    }

    async fn add_member(db: &SqlitePool, trip_id: &str, user_id: &str) {
        sqlx::query(
            "INSERT INTO users (id, email, display_name, password_hash) VALUES (?, ?, ?, ?)",
        )
        .bind(user_id)
        .bind(format!("{user_id}@example.com"))
        .bind("Member")
        .bind("test-hash")
        .execute(db)
        .await
        .unwrap();
        sqlx::query("INSERT INTO trip_members (trip_id, user_id, role) VALUES (?, ?, 'member')")
            .bind(trip_id)
            .bind(user_id)
            .execute(db)
            .await
            .unwrap();
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
        add_member(&state.db, &trip_id, &voter_id).await;
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
        };
        assert_eq!(
            vote(
                State(state.clone()),
                Path(trip_id.clone()),
                authenticated_user(voter_id.clone()),
                csrf_headers(),
                Json(input),
            )
            .await
            .unwrap(),
            StatusCode::CREATED
        );

        let duplicate = vote(
            State(state),
            Path(trip_id),
            authenticated_user(voter_id),
            csrf_headers(),
            Json(VoteRequest { stop_id }),
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
            nexconn: NexconnState::new(db.clone(), "http://localhost:3000".into()),
            db: db.clone(),
            app_origin: "http://localhost:3000".into(),
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

        let voters = (0..100)
            .map(|_| Uuid::new_v4().to_string())
            .collect::<Vec<_>>();
        for voter in &voters {
            add_member(&db, &trip_id, voter).await;
        }

        let writes = voters.iter().map(|voter| {
            let state = state.clone();
            let trip_id = trip_id.clone();
            let stop_id = stop_id.clone();
            let voter_id = voter.clone();
            tokio::spawn(async move {
                vote(
                    State(state),
                    Path(trip_id),
                    authenticated_user(voter_id),
                    csrf_headers(),
                    Json(VoteRequest { stop_id }),
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
        add_member(&db, &trip_id, &voter_id).await;
        let duplicate_writes = (0..10).map(|_| {
            let state = state.clone();
            let trip_id = trip_id.clone();
            let stop_id = stop_id.clone();
            let voter_id = voter_id.clone();
            tokio::spawn(async move {
                vote(
                    State(state),
                    Path(trip_id),
                    authenticated_user(voter_id),
                    csrf_headers(),
                    Json(VoteRequest { stop_id }),
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

    #[tokio::test]
    async fn two_browser_vote_flow_broadcasts_then_refetches_authoritative_counts() {
        use futures_util::StreamExt;
        use tokio::time::{Duration, timeout};
        use tokio_tungstenite::connect_async;

        let state = test_state().await;
        let trip_id = Uuid::new_v4().to_string();
        let other_trip_id = Uuid::new_v4().to_string();
        let stop_id = Uuid::new_v4().to_string();
        for id in [&trip_id, &other_trip_id] {
            sqlx::query("INSERT INTO trips (id, name, destination) VALUES (?, ?, ?)")
                .bind(id)
                .bind("Shared test trip")
                .bind("Test city")
                .execute(&state.db)
                .await
                .unwrap();
        }
        sqlx::query("INSERT INTO trip_stops (id, trip_id, place_id, name) VALUES (?, ?, ?, ?)")
            .bind(&stop_id)
            .bind(&trip_id)
            .bind("place-1")
            .bind("Test place")
            .execute(&state.db)
            .await
            .unwrap();
        let user_a = Uuid::new_v4().to_string();
        let user_b = Uuid::new_v4().to_string();
        add_member(&state.db, &trip_id, &user_a).await;
        add_member(&state.db, &trip_id, &user_b).await;
        assert_eq!(
            require_member(&state.db, &other_trip_id, &user_a)
                .await
                .unwrap_err()
                .0,
            StatusCode::FORBIDDEN
        );
        let token_a = Uuid::new_v4().simple().to_string();
        let token_b = Uuid::new_v4().simple().to_string();
        let csrf_a = Uuid::new_v4().simple().to_string();
        let csrf_b = Uuid::new_v4().simple().to_string();
        for (token, csrf, user) in [(&token_a, &csrf_a, &user_a), (&token_b, &csrf_b, &user_b)] {
            sqlx::query("INSERT INTO sessions (token_hash, user_id, csrf_token, expires_at, created_at) VALUES (?, ?, ?, ?, ?)")
                .bind(crate::auth::hash_token(token)).bind(user).bind(csrf).bind(i64::MAX).bind(0_i64)
                .execute(&state.db).await.unwrap();
        }

        let app = router(state.clone())
            .merge(crate::realtime::router(state.realtime.clone()))
            .layer(axum::middleware::from_fn_with_state(
                state.db.clone(),
                crate::auth::attach_session,
            ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let url = format!("ws://{address}/api/trips/{trip_id}/ws");
        let (mut browser_a, _) = connect_async(&url).await.unwrap();
        let (mut browser_b, _) = connect_async(&url).await.unwrap();
        let (mut other_trip, _) =
            connect_async(format!("ws://{address}/api/trips/{other_trip_id}/ws"))
                .await
                .unwrap();

        for socket in [&mut browser_a, &mut browser_b, &mut other_trip] {
            let sync = timeout(Duration::from_secs(2), socket.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap()
                .into_text()
                .unwrap();
            assert!(sync.contains("sync_required"));
        }

        let client = reqwest::Client::new();
        let unauthorized = client
            .post(format!("http://{address}/api/trips/{trip_id}/votes"))
            .json(&serde_json::json!({ "stop_id": stop_id, "voter_id": "forged-user" }))
            .send()
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
        let vote_response = client
            .post(format!("http://{address}/api/trips/{trip_id}/votes"))
            .header(
                "cookie",
                format!("{}={token_a}", crate::auth::SESSION_COOKIE),
            )
            .header("origin", "http://localhost:3000")
            .header("x-csrf-token", &csrf_a)
            .json(&serde_json::json!({ "stop_id": stop_id, "voter_id": "forged-user" }))
            .send()
            .await
            .unwrap();
        assert_eq!(vote_response.status(), StatusCode::CREATED);
        assert!(vote_response.bytes().await.unwrap().is_empty());
        let stored_voter: String =
            sqlx::query_scalar("SELECT voter_id FROM votes WHERE stop_id = ?")
                .bind(&stop_id)
                .fetch_one(&state.db)
                .await
                .unwrap();
        assert_eq!(stored_voter, user_a);

        for socket in [&mut browser_a, &mut browser_b] {
            let event = timeout(Duration::from_secs(2), socket.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap()
                .into_text()
                .unwrap();
            assert!(event.contains("vote_added"));
            assert!(event.contains(&trip_id));
        }
        assert!(
            timeout(Duration::from_millis(100), other_trip.next())
                .await
                .is_err()
        );

        let second_response = client
            .post(format!("http://{address}/api/trips/{trip_id}/votes"))
            .header(
                "cookie",
                format!("{}={token_b}", crate::auth::SESSION_COOKIE),
            )
            .header("origin", "http://localhost:3000")
            .header("x-csrf-token", &csrf_b)
            .json(&serde_json::json!({ "stop_id": stop_id }))
            .send()
            .await
            .unwrap();
        assert_eq!(second_response.status(), StatusCode::CREATED);
        assert!(second_response.bytes().await.unwrap().is_empty());
        for socket in [&mut browser_a, &mut browser_b] {
            let event = timeout(Duration::from_secs(2), socket.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap()
                .into_text()
                .unwrap();
            assert!(event.contains("vote_added"));
        }

        let stops: Vec<Stop> = client
            .get(format!("http://{address}/api/trips/{trip_id}/stops"))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(stops[0].votes, 2);

        let duplicate = client
            .post(format!("http://{address}/api/trips/{trip_id}/votes"))
            .header(
                "cookie",
                format!("{}={token_b}", crate::auth::SESSION_COOKIE),
            )
            .header("origin", "http://localhost:3000")
            .header("x-csrf-token", &csrf_b)
            .json(&serde_json::json!({ "stop_id": stop_id }))
            .send()
            .await
            .unwrap();
        assert_eq!(duplicate.status(), StatusCode::CONFLICT);
        assert!(duplicate.text().await.unwrap().contains("Already voted"));
        assert!(
            timeout(Duration::from_millis(100), browser_a.next())
                .await
                .is_err()
        );

        server.abort();
    }
}
