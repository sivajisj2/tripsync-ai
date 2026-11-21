# TripSync AI

TripSync is a Rust/Axum and Next.js travel planning app. The API persists trips, stops, votes, preferences, and reviewed itinerary plans in SQLite. Google Maps discovery is fetched from SerpApi and cached in memory; trip planning data is persisted.

## Architecture

- `apps/api`: Axum/Tokio JSON API, SQLite via SQLx, versioned migrations, per-trip Tokio broadcast channels, SerpApi client/cache, recommendations, and smart plan persistence.
- `apps/web`: Next.js App Router frontend, with the warm ivory/forest editorial design and a same-origin `/api` rewrite to the Rust API.
- SQLite runs in WAL mode with foreign keys enabled and a 5-second busy timeout. API write paths use database transactions where a multi-row update must be atomic.
- `GET /metrics` exposes request/error totals and average request/recommendation latency, SerpApi cache hits/misses/errors. Responses include an `x-request-id`; structured request records include that ID, method, path, status, and latency.

## Setup

1. Configure `apps/api/.env` with `SERPAPI_KEY=<your-key>`.
2. Start the API: `cd apps/api && cargo run`.
3. Start the web app in another terminal: `cd apps/web && npm run dev`.
4. Open `http://localhost:3000`.

Set `NEXT_PUBLIC_API_URL` for the API origin when it is not `http://localhost:8080`. The SerpApi key is only read by the Rust backend.

## Smart Ranking

Recommendations are built from current SerpApi results for tourist attractions, restaurants, hotels, and cafes. Duplicate places are merged, saved places remain visible with an “Already in itinerary” state, and no sample destinations or place records are generated.

The deterministic score is 0–100:

```text
score = 100 * (
  0.30 * normalized_rating
  + 0.30 * normalized_trip_category_preference
  + 0.25 * min(group_votes, 10) / 10
  + 0.15 * coordinate_availability
)
```

Ratings normalize against 5 stars. Missing ratings and coordinate pairs receive neutral `0.5` signals and the explanation labels those missing values. Unset category preferences also use a neutral `0.5`. Vote weight is capped at 10. Stable tie-breaking uses score, rating, name, and place ID. The UI displays the score inputs and signals in plain language.

The itinerary generator orders saved stops by rating/preference/vote score, then uses nearest-neighbor ordering when coordinates are available. It saves a draft, not an immediate itinerary mutation. Users can reorder and persist the draft; applying it requires confirmation and updates itinerary positions inside a SQLite transaction. It does not estimate travel times, distances, or availability.

## API

- `GET /api/places/search?city=...&category=...` — real SerpApi places; category is one of `tourist attractions`, `restaurants`, `hotels`, `cafes`.
- `POST /api/trips`, `GET /api/trips/{id}` — create/read trips.
- `GET|POST /api/trips/{id}/stops`, `POST /api/trips/{id}/votes` — itinerary and voting.
- `GET /api/trips/{id}/recommendations` — live, deterministically ranked discovery results for the trip destination.
- `GET|PUT /api/trips/{id}/preferences` — read/write per-trip category weights (0–5).
- `GET|POST /api/trips/{id}/smart-plan` — get the latest persisted draft or generate one from saved stops.
- `PUT /api/trips/{id}/smart-plan/{plan_id}` — reorder draft items with a complete stop-ID list.
- `POST /api/trips/{id}/smart-plan/{plan_id}/apply` — transactionally apply a reviewed order.
- `GET /api/trips/{id}/ws` — trip-isolated WebSocket updates, including `stop_added`, `vote_added`, and `plan_applied`.
- `GET /metrics` — in-memory process metrics; counters reset when the API restarts.

This demo has shareable trip IDs and browser UUID voter IDs, not authenticated membership. Do not treat a trip link or voter identifier as access control.

## Validation

From `apps/api`:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

From `apps/web`:

```bash
npm run lint
npx tsc --noEmit
npm run build
```

The backend tests cover deterministic score fallbacks, nearest-neighbor ordering, plan draft persistence and explicit apply, trip event isolation, duplicate votes, and 100 concurrent unique-member votes with duplicate submissions.

## Load Tests

Install [k6](https://grafana.com/docs/k6/latest/set-up/install-k6/) and run the API with a valid SerpApi key. Create a trip and stop in the app, then supply their real IDs:

```bash
cd apps/api
BASE_URL=http://localhost:8080 \
TRIP_ID=<existing-trip-uuid> \
STOP_ID=<existing-stop-uuid> \
k6 run k6/step5.js
```

The script drives trip reads, real SerpApi-backed recommendations, and unique-voter writes against that trip. It writes votes to SQLite, so use a disposable benchmark trip/database. k6 prints measured request duration/throughput/error summaries. No benchmark numbers are checked in because results depend on the machine, network, SerpApi response/cache state, and k6 settings; record the command output with the environment when reporting measurements.
