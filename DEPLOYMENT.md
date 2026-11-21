# Deployment

## Public free demo with Cloudflare Quick Tunnel

This runs the complete Next.js frontend, Rust/Axum API, WebSockets, SQLite, SerpApi integration, and Nexconn integration on one Ubuntu laptop. Cloudflare Quick Tunnel supplies a temporary HTTPS hostname at no charge. It is not permanent hosting: the hostname can change and the tunnel disappears when the process or laptop stops.

Prerequisites: Docker Engine/Compose, outbound internet, `SERPAPI_KEY`, and (for live chat) `NEXCONN_APP_KEY` plus `NEXCONN_APP_SECRET`. Put server-only credentials in `apps/api/.env` or export them in the shell. The web container never receives the SerpApi key or Nexconn secret. If the existing env file uses `NEXT_PUBLIC_NEXCONN_APP_KEY`, the start script maps it to the server-side key for compatibility.

Install `cloudflared` from Cloudflare's official instructions: <https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/>.

### Start

```bash
./scripts/start-public-demo.sh
```

The script builds and starts the complete Compose stack, starts `cloudflared tunnel --url http://localhost:3000`, captures the assigned `https://*.trycloudflare.com` URL, then recreates only the API with that exact `APP_ORIGIN`. This makes CORS, secure cookies, same-origin `/api` rewrites, and `wss://` upgrades work on the dynamic hostname.

SQLite is stored in the named `tripsync-data` Docker volume. API and web are not published directly; only the tunnel reaches web on localhost:3000.

### Stop, restart, inspect

```bash
./scripts/stop-public-demo.sh
./scripts/start-public-demo.sh
docker compose logs -f api web
tail -f .public-demo/cloudflared.log
cat .public-demo/url
docker compose ps
```

Stopping preserves SQLite. Do not remove the volume unless you intentionally want to delete the data.

### Local checks

```bash
cd apps/api && cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test
cd ../web && npm run lint && npm run test:unit && npx tsc --noEmit && npm run build
```

See `DEMO_CHECKLIST.md` for real two-browser verification. SerpApi and Nexconn require valid live credentials and are not replaced by mocks.

### Limitations and troubleshooting

- Quick Tunnel is temporary and best-effort; keep the Ubuntu laptop powered on, awake, online, and running Docker.
- The public hostname changes after a new tunnel. Always use the URL printed by the start script.
- If the API reports origin errors after a restart, stop and start again so it receives the current hostname.
- If no URL appears, inspect `.public-demo/cloudflared.log` and confirm outbound HTTPS access.
- If chat fails, verify Nexconn credentials, data-center API base URL, and permissions.
- SQLite is single-node storage; back up the Docker volume before destructive maintenance.

### Nexconn region and chat errors

Set `NEXCONN_API_BASE_URL` in `apps/api/.env` to the region shown beside the selected app in the Nexconn console. Use the matching App Key and App Secret from that same application:

- Singapore: `https://api.sg-light-api.com`
- North America: `https://api.us-light-api.com`
- Saudi Arabia: `https://api.sau-light-api.com`

The API returns the matching SDK region with each chat session; the browser does not need a public secret or region environment variable. A key used against the wrong region can return HTTP 403. Group mappings are scoped to the App Key, so changing applications creates groups in the selected application. Messages in the previous provider application are not transferred.

After configuration changes, rebuild before restarting:

```bash
docker compose --env-file .public-demo/compose.env build
./scripts/start-public-demo.sh
```

A Cloudflare 502 can occur while the local frontend restarts. Check `docker compose --env-file .public-demo/compose.env ps` and `/ready`, then reload the page or use chat Retry. The chat mount remains available after errors so Retry can reconnect. Keep secrets out of screenshots and regenerate any exposed App Secret in the console, then update the private API env file.

Chat UI can briefly return error `39005` after SDK sign-in while channel and offline-message sync completes. TripSync waits up to 30 seconds for that sync before reporting an error.
