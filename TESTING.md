# Testing

## Automated

```bash
cd apps/api
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test

cd ../web
npm run lint
npm run test:unit
npx tsc --noEmit
npm run build
```

Rust tests cover authentication/expiry/CSRF, fail-closed Nexconn boundaries, real two-client trip WebSocket broadcasts and refetch counts, trip isolation, unauthorized voting, duplicate votes, 100 concurrent unique voters, smart ranking fallbacks, and persisted itinerary drafts. Node unit tests cover empty successful HTTP bodies, JSON success, malformed JSON, and error responses.

## Manual Two-Browser Flow

1. Configure a real SerpApi key and run API plus web app. Use two independent browser profiles.
2. Register User A, create a trip, search places, add stops, vote, generate a smart draft, reorder, and confirm apply.
3. Copy the shared URL; in Browser B, register User B and open it. Confirm public trip/stops load, then join.
4. Vote from both profiles; verify counts update immediately. Refresh both and confirm counts/order persist.
5. Toggle network offline/online or restart the API; verify `sync_required` and stops refetch after WebSocket reconnect.
6. With Nexconn credentials configured, confirm both users join the same group, exchange messages, refresh history, and recover after reconnect. Without credentials the UI must report chat unavailable, not show simulated messages.
7. Sign out and confirm protected writes return 401. Try a logged-in nonmember and confirm protected trip writes return 403.

Live two-user browser validation has not been run in this environment. Nexconn messaging additionally requires valid App Key/Secret and Console app configuration.

## Load Tests

Install k6 and create a disposable trip/stop. See `apps/api/README.md` for the command using `BASE_URL`, `TRIP_ID`, and `STOP_ID`. Voting writes persist, and recommendation calls contact SerpApi; do not point the script at production data without approval. Record the k6 output and environment; no benchmark numbers are asserted here.
