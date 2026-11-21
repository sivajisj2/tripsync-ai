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

## Public chat verification — 2026-11-21

Verified through the active public HTTPS Quick Tunnel using two isolated Chromium browser contexts and the live Nexconn North America application:

- Registered two independent users, created a dedicated verification trip, and joined the second user.
- Both chat-session endpoints returned 200 and both clients opened the group.
- Sent a real Nexconn message from A to B and a reply from B to A.
- Refreshed B and confirmed the message history remained visible.
- Separately injected one HTML 502 into the first chat-session request and confirmed Retry recovered. Messaging verification used the real provider.

Rust formatting, clippy with warnings denied, all 10 Rust tests, frontend lint, all 7 frontend unit tests, TypeScript, and production builds passed for the chat fix.
