# Architecture

## Components

- Next.js App Router serves the premium editorial UI and same-origin `/api` rewrite.
- Rust Axum/Tokio API owns authentication, session cookies, trip authorization, SerpApi requests, SQLite transactions, metrics, and WebSocket channels.
- SQLite runs with WAL, foreign keys, busy timeout, and versioned SQLx migrations. Production Compose mounts `/data` on a persistent named volume.
- SerpApi is called only by Rust; its key is never included in frontend configuration.
- Nexconn ChatUI renders group history and messaging. The API signs Nexconn Platform API requests and issues user tokens; the browser receives a short-lived/rotatable provider access token only after app-session and trip-membership checks.

## Authentication and Trust

Registration/login hashes passwords with Argon2id. The server stores only a SHA-256 hash of an opaque session token, returns the token in an HttpOnly, SameSite=Lax cookie, and checks expiry on every request. Mutating browser requests require the session's CSRF token and exact configured Origin. Production requires `COOKIE_SECURE=true` behind HTTPS.

Trip creation assigns the authenticated creator as owner. Shared trip IDs permit public read-only viewing; a signed-in user must join before adding stops, voting, editing preferences, changing plans, or requesting a chat token. Vote identity comes from the session. WebSockets remain trip-scoped; reconnect receives `sync_required` and triggers a fresh read.

## Nexconn Group Mapping

Each trip lazily maps to one deterministic, persisted Nexconn group ID (`nexconn_trip_groups`). Nexconn user IDs are derived from opaque local UUIDs, not sequential identifiers. The API issues Nexconn access tokens and creates/joins groups with signed HTTPS Platform API requests. ChatUI `ServiceHooks` fetch actual local user/group/member records. Nexconn history/messages remain provider-owned; TripSync does not fake or duplicate them.

Nexconn provider credentials are absent here, so successful live token issuance, group creation, and message exchange remain unverified. The SDK dependency and integration paths are present, and missing credentials fail closed with a clear setup error.

## Smart Planning

Score (0–100) = 30% normalized rating + 30% category preference + 25% capped group votes + 15% coordinate availability. Missing rating/location uses a documented neutral input. Recommendations come from live SerpApi. Draft plans persist in SQLite; applying a plan explicitly and transactionally updates stop order. Coordinates are used only for nearest-neighbor ordering; no travel-time or distance claims are produced.

## Operational Visibility

Every HTTP response has `x-request-id`; structured logs include request ID, method, path, status, and duration. `/health` is liveness, `/ready` checks SQLite, and `/metrics` exposes process-local counts and average latencies. Metrics reset on restart and are not a replacement for a durable monitoring backend.
