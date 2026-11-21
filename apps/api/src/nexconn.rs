use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha1::{Digest, Sha1};
use sqlx::SqlitePool;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{
    auth::{self, AuthenticatedUser, nexconn_user_id},
    trips::require_member,
};

#[derive(Clone)]
pub struct NexconnConfig {
    app_key: String,
    app_secret: String,
    api_base: String,
}

impl NexconnConfig {
    pub fn from_env() -> Option<Self> {
        let app_key = std::env::var("NEXCONN_APP_KEY").ok()?;
        let app_secret = std::env::var("NEXCONN_APP_SECRET").ok()?;
        let api_base = std::env::var("NEXCONN_API_BASE_URL")
            .unwrap_or_else(|_| "https://api.sg-light-api.com".into());
        let parsed = reqwest::Url::parse(&api_base).ok()?;
        if parsed.scheme() != "https"
            || parsed.host_str().is_none()
            || app_key.is_empty()
            || app_secret.is_empty()
        {
            return None;
        }
        Some(Self {
            app_key,
            app_secret,
            api_base: api_base.trim_end_matches('/').to_owned(),
        })
    }
}

#[derive(Clone)]
pub struct NexconnState {
    pub db: SqlitePool,
    config: Option<NexconnConfig>,
    client: reqwest::Client,
    create_lock: Arc<Mutex<()>>,
    pub app_origin: String,
}

impl NexconnState {
    pub fn new(db: SqlitePool, app_origin: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(7))
            .pool_max_idle_per_host(0)
            .build()
            .expect("valid Nexconn HTTP client configuration");
        Self {
            db,
            config: NexconnConfig::from_env(),
            client,
            create_lock: Arc::new(Mutex::new(())),
            app_origin,
        }
    }

    pub fn configured(&self) -> bool {
        self.config.is_some()
    }

    fn unavailable() -> (StatusCode, String) {
        (StatusCode::SERVICE_UNAVAILABLE, "Nexconn chat is not configured. Set NEXCONN_APP_KEY, NEXCONN_APP_SECRET, and NEXCONN_API_BASE_URL on the API server.".into())
    }

    async fn post<T: DeserializeOwned>(
        &self,
        path: &str,
        payload: serde_json::Value,
    ) -> Result<Option<T>, (StatusCode, String)> {
        let config = self.config.as_ref().ok_or_else(Self::unavailable)?;
        let nonce = Uuid::new_v4().simple().to_string()[..18].to_owned();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .to_string();
        let signature = format!(
            "{:x}",
            Sha1::digest(format!("{}{}{}", config.app_secret, nonce, timestamp).as_bytes())
        );
        let url = format!("{}{path}", config.api_base);
        let response = self
            .client
            .post(url)
            .header("App-Key", &config.app_key)
            .header("Nonce", nonce)
            .header("Timestamp", timestamp)
            .header("Signature", signature)
            .header("X-Request-ID", Uuid::new_v4().to_string())
            .json(&payload)
            .send()
            .await
            .map_err(|error| {
                tracing::warn!(%error, path, "Nexconn API request failed");
                (
                    StatusCode::BAD_GATEWAY,
                    "Nexconn service is unavailable".into(),
                )
            })?;
        if !response.status().is_success() {
            tracing::warn!(status = %response.status(), path, "Nexconn API returned an error");
            return Err((
                StatusCode::BAD_GATEWAY,
                "Nexconn rejected the request".into(),
            ));
        }
        let envelope: ApiEnvelope<T> = response.json().await.map_err(|error| {
            tracing::warn!(%error, path, "Nexconn response was invalid");
            (
                StatusCode::BAD_GATEWAY,
                "Nexconn returned an invalid response".into(),
            )
        })?;
        if envelope.code != 0 {
            tracing::warn!(
                nexconn_code = envelope.code,
                path,
                "Nexconn operation failed"
            );
            return Err((StatusCode::BAD_GATEWAY, "Nexconn operation failed".into()));
        }
        Ok(envelope.result)
    }

    async fn issue_token(&self, user: &AuthenticatedUser) -> Result<String, (StatusCode, String)> {
        #[derive(Deserialize)]
        struct TokenResult {
            #[serde(rename = "accessToken")]
            access_token: String,
        }
        let result: TokenResult = self
            .post(
                "/v4/auth/access-token/issue",
                serde_json::json!({
                    "userId": nexconn_user_id(&user.id), "name": user.display_name.chars().take(64).collect::<String>(),
                }),
            )
            .await?
            .ok_or((
                StatusCode::BAD_GATEWAY,
                "Nexconn did not return a user token".into(),
            ))?;
        Ok(result.access_token)
    }

    async fn ensure_group(&self, trip_id: &str) -> Result<String, (StatusCode, String)> {
        let config = self.config.as_ref().ok_or_else(Self::unavailable)?;
        // Remote groups belong to an App Key. Recreate the mapping if credentials change.
        let channel_id = format!("TS{}{}", config.app_key, trip_id.replace('-', ""));
        if let Some(id) = sqlx::query_scalar::<_, String>(
            "SELECT channel_id FROM nexconn_trip_groups WHERE trip_id = ? AND channel_id = ?",
        )
        .bind(trip_id)
        .bind(&channel_id)
        .fetch_optional(&self.db)
        .await
        .map_err(db_error)?
        {
            return Ok(id);
        }
        let _guard = self.create_lock.lock().await;
        if let Some(id) = sqlx::query_scalar::<_, String>(
            "SELECT channel_id FROM nexconn_trip_groups WHERE trip_id = ? AND channel_id = ?",
        )
        .bind(trip_id)
        .bind(&channel_id)
        .fetch_optional(&self.db)
        .await
        .map_err(db_error)?
        {
            return Ok(id);
        }
        #[derive(sqlx::FromRow)]
        struct TripRow {
            name: String,
            owner_id: String,
            owner_name: String,
        }
        let trip = sqlx::query_as::<_, TripRow>(
            "SELECT t.name, m.user_id AS owner_id, u.display_name AS owner_name FROM trips t
             JOIN trip_members m ON m.trip_id = t.id
             JOIN users u ON u.id = m.user_id WHERE t.id = ?
             ORDER BY CASE WHEN m.role = 'owner' THEN 0 ELSE 1 END, m.joined_at LIMIT 1",
        )
        .bind(trip_id)
        .fetch_optional(&self.db)
        .await
        .map_err(db_error)?
        .ok_or((StatusCode::NOT_FOUND, "Trip owner not found".into()))?;
        let owner = AuthenticatedUser {
            id: trip.owner_id.clone(),
            email: String::new(),
            display_name: trip.owner_name,
            csrf_token: String::new(),
            session_hash: String::new(),
        };
        let _ = self.issue_token(&owner).await?;
        let group_name = trip.name.chars().take(64).collect::<String>();
        let _: Option<serde_json::Value> = self.post("/v4/group-channel/create", serde_json::json!({
            "channelId": channel_id, "name": group_name, "owner": nexconn_user_id(&trip.owner_id),
            "permissions": { "joinPerm": 3, "memInvitePerm": 0, "profilePerm": 0 }
        })).await?;
        let mut tx = self.db.begin().await.map_err(db_error)?;
        sqlx::query("DELETE FROM nexconn_group_members WHERE trip_id = ?")
            .bind(trip_id)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        sqlx::query("INSERT INTO nexconn_trip_groups (trip_id, channel_id) VALUES (?, ?) ON CONFLICT(trip_id) DO UPDATE SET channel_id = excluded.channel_id")
            .bind(trip_id)
            .bind(&channel_id)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        sqlx::query("INSERT INTO nexconn_group_members (trip_id, user_id) VALUES (?, ?) ON CONFLICT(trip_id, user_id) DO NOTHING")
            .bind(trip_id).bind(&trip.owner_id).execute(&mut *tx).await.map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(channel_id)
    }

    async fn ensure_member(
        &self,
        trip_id: &str,
        user: &AuthenticatedUser,
    ) -> Result<String, (StatusCode, String)> {
        require_member(&self.db, trip_id, &user.id)
            .await
            .map_err(|(status, message)| (status, message.into()))?;
        let channel_id = self.ensure_group(trip_id).await?;
        let joined = sqlx::query_scalar::<_, i64>(
            "SELECT EXISTS(SELECT 1 FROM nexconn_group_members WHERE trip_id = ? AND user_id = ?)",
        )
        .bind(trip_id)
        .bind(&user.id)
        .fetch_one(&self.db)
        .await
        .map_err(db_error)?;
        if joined == 0 {
            let _ = self.issue_token(user).await?;
            let _: Option<serde_json::Value> = self
                .post(
                    "/v4/group-channel/join",
                    serde_json::json!({
                        "channelId": channel_id, "userIds": [nexconn_user_id(&user.id)]
                    }),
                )
                .await?;
            sqlx::query("INSERT INTO nexconn_group_members (trip_id, user_id) VALUES (?, ?) ON CONFLICT(trip_id, user_id) DO NOTHING")
                .bind(trip_id).bind(&user.id).execute(&self.db).await.map_err(db_error)?;
        }
        Ok(channel_id)
    }
}

#[derive(Deserialize)]
struct ApiEnvelope<T> {
    code: i64,
    result: Option<T>,
}

#[derive(Serialize)]
struct ChatSessionResponse {
    app_key: String,
    area_code: u8,
    user_id: String,
    group_id: String,
    token: String,
}

#[derive(Deserialize)]
struct IdQuery {
    ids: String,
}

#[derive(Serialize)]
struct UserProfile {
    #[serde(rename = "userId")]
    user_id: String,
    name: String,
}

#[derive(Serialize)]
struct GroupProfile {
    #[serde(rename = "groupId")]
    group_id: String,
    name: String,
    #[serde(rename = "memberCount")]
    member_count: i64,
}

#[derive(Serialize)]
struct MemberProfile {
    #[serde(rename = "userId")]
    user_id: String,
    nickname: String,
}

async fn chat_session(
    State(state): State<NexconnState>,
    Path(trip_id): Path<String>,
    user: AuthenticatedUser,
    headers: axum::http::HeaderMap,
) -> Result<Json<ChatSessionResponse>, (StatusCode, String)> {
    auth::verify_csrf(&headers, &user, &state.app_origin)?;
    let group_id = state.ensure_member(&trip_id, &user).await?;
    let token = state.issue_token(&user).await?;
    let app_key = state
        .config
        .as_ref()
        .ok_or_else(NexconnState::unavailable)?
        .app_key
        .clone();
    Ok(Json(ChatSessionResponse {
        app_key,
        area_code: match reqwest::Url::parse(
            &state
                .config
                .as_ref()
                .ok_or_else(NexconnState::unavailable)?
                .api_base,
        )
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .as_deref()
        {
            Some("api.us-light-api.com" | "api-b.us-light-api.com") => 3,
            Some("api.sau-light-api.com" | "api-b.sau-light-api.com") => 5,
            _ => 2,
        },
        user_id: nexconn_user_id(&user.id),
        group_id,
        token,
    }))
}

async fn user_profiles(
    State(state): State<NexconnState>,
    user: AuthenticatedUser,
    Query(query): Query<IdQuery>,
) -> Result<Json<Vec<UserProfile>>, (StatusCode, String)> {
    if query.ids.len() > 4096 {
        return Err((StatusCode::BAD_REQUEST, "Too many profile IDs".into()));
    }
    let ids = query
        .ids
        .split(',')
        .take(50)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT DISTINCT u.id, u.display_name FROM users u
         JOIN trip_members target ON target.user_id = u.id
         WHERE target.trip_id IN (SELECT trip_id FROM trip_members WHERE user_id = ?)",
    )
    .bind(&user.id)
    .fetch_all(&state.db)
    .await
    .map_err(db_error)?;
    let profiles = rows
        .into_iter()
        .filter_map(|(id, name)| {
            ids.contains(&nexconn_user_id(&id)).then_some(UserProfile {
                user_id: nexconn_user_id(&id),
                name,
            })
        })
        .collect();
    Ok(Json(profiles))
}

async fn group_profiles(
    State(state): State<NexconnState>,
    user: AuthenticatedUser,
    Query(query): Query<IdQuery>,
) -> Result<Json<Vec<GroupProfile>>, (StatusCode, String)> {
    if query.ids.len() > 4096 {
        return Err((StatusCode::BAD_REQUEST, "Too many group IDs".into()));
    }
    let ids = query
        .ids
        .split(',')
        .take(50)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let rows = sqlx::query_as::<_, (String, String, String, i64)>(
        "SELECT g.channel_id, t.name, g.trip_id, COUNT(m.user_id) FROM nexconn_trip_groups g
         JOIN trips t ON t.id = g.trip_id LEFT JOIN trip_members m ON m.trip_id = g.trip_id
         WHERE g.trip_id IN (SELECT trip_id FROM trip_members WHERE user_id = ?)
         GROUP BY g.trip_id",
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await
    .map_err(db_error)?;
    Ok(Json(
        rows.into_iter()
            .filter(|(id, _, _, _)| ids.contains(id))
            .map(|(group_id, name, _, member_count)| GroupProfile {
                group_id,
                name,
                member_count,
            })
            .collect(),
    ))
}

async fn group_members(
    State(state): State<NexconnState>,
    Path(group_id): Path<String>,
    user: AuthenticatedUser,
) -> Result<Json<Vec<MemberProfile>>, (StatusCode, String)> {
    let trip_id = sqlx::query_scalar::<_, String>(
        "SELECT trip_id FROM nexconn_trip_groups WHERE channel_id = ?",
    )
    .bind(&group_id)
    .fetch_optional(&state.db)
    .await
    .map_err(db_error)?
    .ok_or((StatusCode::NOT_FOUND, "Chat group not found".into()))?;
    require_member(&state.db, &trip_id, &user.id)
        .await
        .map_err(|(status, message)| (status, message.into()))?;
    let rows = sqlx::query_as::<_, (String, String)>("SELECT u.id, u.display_name FROM trip_members m JOIN users u ON u.id = m.user_id WHERE m.trip_id = ? ORDER BY m.joined_at")
        .bind(trip_id).fetch_all(&state.db).await.map_err(db_error)?;
    Ok(Json(
        rows.into_iter()
            .map(|(id, nickname)| MemberProfile {
                user_id: nexconn_user_id(&id),
                nickname,
            })
            .collect(),
    ))
}

fn db_error(error: sqlx::Error) -> (StatusCode, String) {
    tracing::error!(%error, "Nexconn integration database error");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "Chat integration failed".into(),
    )
}

pub fn router(state: NexconnState) -> Router {
    Router::new()
        .route("/api/trips/{id}/chat/session", post(chat_session))
        .route("/api/chat/profiles/users", get(user_profiles))
        .route("/api/chat/profiles/groups", get(group_profiles))
        .route(
            "/api/chat/profiles/groups/{group_id}/members",
            get(group_members),
        )
        .with_state(state)
}

pub async fn sync_new_trip_member(
    state: &NexconnState,
    trip_id: &str,
    user: &AuthenticatedUser,
) -> Result<(), (StatusCode, String)> {
    if !state.configured() {
        return Ok(());
    }
    state.ensure_member(trip_id, user).await.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    #[tokio::test]
    async fn chat_fails_closed_without_provider_credentials() {
        let db = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let state = NexconnState {
            db,
            config: None,
            client: reqwest::Client::new(),
            create_lock: Arc::new(Mutex::new(())),
            app_origin: "http://localhost:3000".into(),
        };
        let error = state
            .post::<serde_json::Value>("/v4/test", serde_json::json!({}))
            .await
            .unwrap_err();
        assert_eq!(error.0, StatusCode::SERVICE_UNAVAILABLE);
        assert!(error.1.contains("NEXCONN_APP_KEY"));
    }
}
