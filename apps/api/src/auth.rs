use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use axum::{
    Json, Router,
    body::Body,
    extract::{FromRequestParts, State},
    http::{HeaderMap, HeaderValue, Request, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, SqlitePool};
use subtle::ConstantTimeEq;
use uuid::Uuid;

pub const SESSION_COOKIE: &str = "tripsync_session";
pub const DEFAULT_SESSION_TTL: u64 = 7 * 24 * 60 * 60;
const RATE_WINDOW: Duration = Duration::from_secs(60);
const RATE_MAX: usize = 10;

#[derive(Clone)]
pub struct AuthState {
    pub db: SqlitePool,
    pub app_origin: String,
    pub secure_cookie: bool,
    pub session_ttl: u64,
    pub rate_limiter: RateLimiter,
}

#[derive(Clone, Default)]
pub struct RateLimiter(Arc<Mutex<HashMap<String, Vec<Instant>>>>);

impl RateLimiter {
    fn allow(&self, key: &str) -> bool {
        let Ok(mut windows) = self.0.lock() else {
            return false;
        };
        let now = Instant::now();
        for attempts in windows.values_mut() {
            attempts.retain(|attempt| now.duration_since(*attempt) < RATE_WINDOW);
        }
        windows.retain(|_, attempts| !attempts.is_empty());
        if windows.len() >= 10_000 && !windows.contains_key(key) {
            return false;
        }
        let attempts = windows.entry(key.to_owned()).or_default();
        if attempts.len() >= RATE_MAX {
            return false;
        }
        attempts.push(now);
        true
    }
}

#[derive(Clone, Debug)]
pub struct AuthenticatedUser {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub csrf_token: String,
    pub session_hash: String,
}

#[derive(Clone, Default)]
pub struct SessionMarker(pub Option<AuthenticatedUser>);

#[derive(Serialize, Deserialize, FromRow, Clone)]
pub struct PublicUser {
    pub id: String,
    pub email: String,
    pub display_name: String,
}

#[derive(Deserialize)]
pub struct Credentials {
    email: String,
    password: String,
}

#[derive(Deserialize)]
pub struct Registration {
    email: String,
    password: String,
    display_name: String,
}

#[derive(Serialize)]
struct AuthPayload {
    user: PublicUser,
    csrf_token: String,
}

#[derive(Serialize)]
struct CurrentSession {
    user: Option<PublicUser>,
    csrf_token: Option<String>,
}

type AuthResult<T> = Result<T, (StatusCode, String)>;

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub fn nexconn_user_id(user_id: &str) -> String {
    format!("ts{}", user_id.replace('-', ""))
}

pub(crate) fn hash_token(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

fn valid_email(email: &str) -> bool {
    if email.len() > 254 || email.trim() != email {
        return false;
    }
    let Some((local, domain)) = email.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && local.len() <= 64
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
}

fn verify_origin(headers: &HeaderMap, expected: &str) -> Result<(), (StatusCode, String)> {
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok());
    if origin == Some(expected) {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            "Request origin is not allowed".into(),
        ))
    }
}

pub fn verify_csrf(
    headers: &HeaderMap,
    user: &AuthenticatedUser,
    expected_origin: &str,
) -> Result<(), (StatusCode, String)> {
    verify_origin(headers, expected_origin)?;
    let Some(candidate) = headers
        .get("x-csrf-token")
        .and_then(|value| value.to_str().ok())
    else {
        return Err((StatusCode::FORBIDDEN, "Missing CSRF token".into()));
    };
    if candidate
        .as_bytes()
        .ct_eq(user.csrf_token.as_bytes())
        .into()
    {
        Ok(())
    } else {
        Err((StatusCode::FORBIDDEN, "Invalid CSRF token".into()))
    }
}

async fn resolve_session(db: &SqlitePool, raw_token: &str) -> Option<AuthenticatedUser> {
    #[derive(FromRow)]
    struct SessionRow {
        id: String,
        email: String,
        display_name: String,
        csrf_token: String,
        token_hash: String,
    }
    sqlx::query_as::<_, SessionRow>(
        "SELECT u.id, u.email, u.display_name, s.csrf_token, s.token_hash
         FROM sessions s JOIN users u ON u.id = s.user_id
         WHERE s.token_hash = ? AND s.expires_at > ?",
    )
    .bind(hash_token(raw_token))
    .bind(unix_now())
    .fetch_optional(db)
    .await
    .ok()?
    .map(|session| AuthenticatedUser {
        id: session.id,
        email: session.email,
        display_name: session.display_name,
        csrf_token: session.csrf_token,
        session_hash: session.token_hash,
    })
}

fn cookie_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|entry| {
            let (name, value) = entry.trim().split_once('=')?;
            (name == SESSION_COOKIE && !value.is_empty()).then(|| value.to_owned())
        })
}

pub async fn attach_session(
    State(db): State<SqlitePool>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let user = match cookie_token(request.headers()).as_deref() {
        Some(token) => resolve_session(&db, token).await,
        None => None,
    };
    request.extensions_mut().insert(SessionMarker(user));
    next.run(request).await
}

impl<S> FromRequestParts<S> for AuthenticatedUser
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _: &S,
    ) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<SessionMarker>()
            .and_then(|marker| marker.0.clone())
            .ok_or((StatusCode::UNAUTHORIZED, "Authentication required"))
    }
}

async fn create_session(
    db: &SqlitePool,
    user: PublicUser,
    state: &AuthState,
) -> AuthResult<(AuthPayload, String)> {
    let token = Uuid::new_v4().simple().to_string();
    let token_hash = hash_token(&token);
    let csrf_token = Uuid::new_v4().simple().to_string();
    let now = unix_now();
    sqlx::query("INSERT INTO sessions (token_hash, user_id, csrf_token, expires_at, created_at) VALUES (?, ?, ?, ?, ?)")
        .bind(&token_hash).bind(&user.id).bind(&csrf_token).bind(now + state.session_ttl as i64).bind(now)
        .execute(db).await.map_err(|error| {
            tracing::error!(%error, "Unable to create login session");
            (StatusCode::INTERNAL_SERVER_ERROR, "Could not create session".into())
        })?;
    Ok((AuthPayload { user, csrf_token }, token))
}

fn set_session_cookie(response: &mut Response, token: &str, state: &AuthState) -> AuthResult<()> {
    let secure = if state.secure_cookie { "; Secure" } else { "" };
    let cookie = format!(
        "{SESSION_COOKIE}={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age={}{}",
        state.session_ttl, secure
    );
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie).map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not set session cookie".into(),
            )
        })?,
    );
    Ok(())
}

async fn register(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<Registration>,
) -> AuthResult<Response> {
    verify_origin(&headers, &state.app_origin)?;
    let email = input.email.trim().to_lowercase();
    let display_name = input.display_name.trim();
    if !valid_email(&email)
        || display_name.is_empty()
        || display_name.len() > 80
        || input.password.len() < 12
        || input.password.len() > 128
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "Enter a valid email, a display name, and a password of 12–128 characters".into(),
        ));
    }
    if !state.rate_limiter.allow(&format!("register:{email}")) {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "Too many registration attempts. Try again later".into(),
        ));
    }
    let password = input.password;
    let password_hash = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|hash| hash.to_string())
            .map_err(|_| "Password hashing failed".into())
    })
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Password hashing failed".into(),
        )
    })?
    .map_err(|message| (StatusCode::INTERNAL_SERVER_ERROR, message))?;

    let user = PublicUser {
        id: Uuid::new_v4().to_string(),
        email,
        display_name: display_name.to_owned(),
    };
    let result = sqlx::query(
        "INSERT INTO users (id, email, display_name, password_hash) VALUES (?, ?, ?, ?)",
    )
    .bind(&user.id)
    .bind(&user.email)
    .bind(&user.display_name)
    .bind(password_hash)
    .execute(&state.db)
    .await;
    match result {
        Ok(_) => {}
        Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
            return Err((
                StatusCode::CONFLICT,
                "An account with this email already exists".into(),
            ));
        }
        Err(error) => {
            tracing::error!(%error, "User registration failed");
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Registration failed".into(),
            ));
        }
    }
    let (payload, token) = create_session(&state.db, user, &state).await?;
    let mut response = (StatusCode::CREATED, Json(payload)).into_response();
    set_session_cookie(&mut response, &token, &state)?;
    Ok(response)
}

async fn login(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<Credentials>,
) -> AuthResult<Response> {
    verify_origin(&headers, &state.app_origin)?;
    let email = input.email.trim().to_lowercase();
    if !valid_email(&email) || input.password.len() > 128 {
        return Err((StatusCode::UNAUTHORIZED, "Invalid email or password".into()));
    }
    if !state.rate_limiter.allow(&format!("login:{email}")) {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "Too many sign-in attempts. Try again later".into(),
        ));
    }
    #[derive(FromRow)]
    struct LoginRow {
        id: String,
        email: String,
        display_name: String,
        password_hash: String,
    }
    let row = sqlx::query_as::<_, LoginRow>(
        "SELECT id, email, display_name, password_hash FROM users WHERE email = ?",
    )
    .bind(&email)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| {
        tracing::error!(%error, "User login lookup failed");
        (StatusCode::INTERNAL_SERVER_ERROR, "Sign in failed".into())
    })?
    .ok_or((StatusCode::UNAUTHORIZED, "Invalid email or password".into()))?;
    let encoded = row.password_hash.clone();
    let password = input.password;
    let valid = tokio::task::spawn_blocking(move || {
        PasswordHash::new(&encoded).is_ok_and(|hash| {
            Argon2::default()
                .verify_password(password.as_bytes(), &hash)
                .is_ok()
        })
    })
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Sign in failed".into()))?;
    if !valid {
        return Err((StatusCode::UNAUTHORIZED, "Invalid email or password".into()));
    }
    let user = PublicUser {
        id: row.id,
        email: row.email,
        display_name: row.display_name,
    };
    let (payload, token) = create_session(&state.db, user, &state).await?;
    let mut response = Json(payload).into_response();
    set_session_cookie(&mut response, &token, &state)?;
    Ok(response)
}

async fn current_session(extension: axum::Extension<SessionMarker>) -> Json<CurrentSession> {
    Json(CurrentSession {
        user: extension.0.0.as_ref().map(|user| PublicUser {
            id: user.id.clone(),
            email: user.email.clone(),
            display_name: user.display_name.clone(),
        }),
        csrf_token: extension.0.0.as_ref().map(|user| user.csrf_token.clone()),
    })
}

async fn logout(
    State(state): State<AuthState>,
    user: AuthenticatedUser,
    headers: HeaderMap,
) -> AuthResult<Response> {
    verify_csrf(&headers, &user, &state.app_origin)?;
    sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
        .bind(user.session_hash)
        .execute(&state.db)
        .await
        .map_err(|error| {
            tracing::error!(%error, "Logout failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "Sign out failed".into())
        })?;
    let secure = if state.secure_cookie { "; Secure" } else { "" };
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!(
            "{SESSION_COOKIE}=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0{secure}"
        ))
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not clear session cookie".into(),
            )
        })?,
    );
    Ok(response)
}

pub fn router(state: AuthState) -> Router {
    Router::new()
        .route("/api/auth/register", post(register))
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/me", get(current_session))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn state() -> AuthState {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&db).await.unwrap();
        AuthState {
            db,
            app_origin: "http://localhost:3000".into(),
            secure_cookie: false,
            session_ttl: 300,
            rate_limiter: RateLimiter::default(),
        }
    }

    fn origin() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("http://localhost:3000"),
        );
        headers
    }

    #[tokio::test]
    async fn registration_duplicate_email_invalid_login_and_expired_session() {
        let state = state().await;
        let first = register(
            State(state.clone()),
            origin(),
            Json(Registration {
                email: "Sailor@example.com".into(),
                password: "correct horse battery".into(),
                display_name: "Sailor".into(),
            }),
        )
        .await
        .unwrap();
        assert_eq!(first.status(), StatusCode::CREATED);
        let cookie = first
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        let token = cookie
            .split(';')
            .next()
            .unwrap()
            .split_once('=')
            .unwrap()
            .1
            .to_owned();
        assert!(resolve_session(&state.db, &token).await.is_some());

        let signed_in = login(
            State(state.clone()),
            origin(),
            Json(Credentials {
                email: "SAILOR@example.com".into(),
                password: "correct horse battery".into(),
            }),
        )
        .await
        .unwrap();
        assert_eq!(signed_in.status(), StatusCode::OK);

        let duplicate = register(
            State(state.clone()),
            origin(),
            Json(Registration {
                email: "sailor@example.com".into(),
                password: "correct horse battery".into(),
                display_name: "Sailor Two".into(),
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(duplicate.0, StatusCode::CONFLICT);

        let invalid = login(
            State(state.clone()),
            origin(),
            Json(Credentials {
                email: "sailor@example.com".into(),
                password: "wrong password".into(),
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(invalid.0, StatusCode::UNAUTHORIZED);

        sqlx::query("UPDATE sessions SET expires_at = 0 WHERE token_hash = ?")
            .bind(hash_token(&token))
            .execute(&state.db)
            .await
            .unwrap();
        assert!(resolve_session(&state.db, &token).await.is_none());
    }

    #[tokio::test]
    async fn protected_extractor_rejects_missing_session_and_csrf_checks_origin() {
        let mut parts = Request::new(()).into_parts().0;
        let error = AuthenticatedUser::from_request_parts(&mut parts, &())
            .await
            .unwrap_err();
        assert_eq!(error.0, StatusCode::UNAUTHORIZED);

        let user = AuthenticatedUser {
            id: "u".into(),
            email: "u@example.com".into(),
            display_name: "User".into(),
            csrf_token: "csrf".into(),
            session_hash: "hash".into(),
        };
        let mut headers = origin();
        headers.insert("x-csrf-token", HeaderValue::from_static("csrf"));
        assert!(verify_csrf(&headers, &user, "http://localhost:3000").is_ok());
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://attacker.example"),
        );
        assert_eq!(
            verify_csrf(&headers, &user, "http://localhost:3000")
                .unwrap_err()
                .0,
            StatusCode::FORBIDDEN
        );
    }
}
