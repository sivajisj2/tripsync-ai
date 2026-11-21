# Public Demo Checklist

Run `./scripts/start-public-demo.sh`, open the printed HTTPS URL, and keep the Ubuntu laptop awake and connected to the internet.

## Verified locally by automated tests

- Rust formatting, clippy, unit/integration tests, authentication, CSRF, trip isolation, concurrent voting, WebSocket broadcast/refetch behavior, smart-plan persistence.
- Next.js lint, TypeScript, unit tests, and production build.

## Verify in the public demo

- [ ] HTTPS frontend loads and browser DevTools contains no `localhost` requests.
- [ ] `/health` and `/ready` return healthy/ready JSON.
- [ ] Register User A, sign in, refresh, and confirm the session persists.
- [ ] Search a destination and confirm real SerpApi place results.
- [ ] Create a trip, add places, copy its share link, and open it in a separate browser profile.
- [ ] Register User B, join the trip, vote from both sessions, and observe the WebSocket update.
- [ ] Generate, reorder, apply, and refresh the smart itinerary.
- [ ] Open Nexconn chat in both sessions, send real messages in both directions, refresh, and confirm provider history.
- [ ] Confirm browser network payloads do not contain `SERPAPI_KEY` or `NEXCONN_APP_SECRET`.

If SerpApi or Nexconn credentials are absent or rejected, mark that item unverified rather than substituting a mock.
