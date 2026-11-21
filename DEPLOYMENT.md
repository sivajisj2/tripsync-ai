# Deployment

## Local Docker Compose

1. Copy `apps/api/.env.example` to `apps/api/.env`, then set `SERPAPI_KEY`; add Nexconn server credentials only if live chat is required.
2. Run `cp compose.env.example .env`.
3. Start: `docker compose -f docker-compose.yml -f docker-compose.local.yml up --build`.
4. Web: `http://localhost:3000`; API readiness: `http://localhost:8080/ready`. The named `tripsync-data` volume persists SQLite.

The production Compose service exposes the API internally only. `docker-compose.local.yml` publishes ports for local WebSocket development and must not be used in production.

## Production Compose

Use a Linux VM/container host with a persistent disk and long-lived WebSocket support. A free-tier VM may be possible but is provider/region/quota dependent and is not guaranteed. Serverless/free web tiers that sleep, disallow WebSockets, or provide ephemeral filesystems are unsuitable for this single-node SQLite deployment. Persistent disks, outbound network access, DNS, and possibly billing are required; confirm provider pricing before provisioning.

1. Point a DNS name to the host and allow inbound TCP 80/443.
2. Configure root `.env` (copied from `compose.env.example`) with real `SERPAPI_KEY`, `NEXCONN_APP_KEY`, `NEXCONN_APP_SECRET`, and the correct Nexconn data-center API base URL.
3. Set `DOMAIN=travel.example.com`, `APP_ORIGIN=https://travel.example.com`, `NEXT_PUBLIC_API_URL=https://travel.example.com`, and `COOKIE_SECURE=true`.
4. Build the web container with those public URL build args and run `docker compose --profile production up -d --build`.
5. Verify `https://travel.example.com/health`, `/ready`, sign-in, a shared trip, a WebSocket reconnect, SerpApi, and Nexconn using two real users.
6. Back up the persistent `/data/tripsync.db` volume and test restore/migration procedures.

Caddy obtains/renews TLS certificates and proxies `/api/*` (including WebSockets) to Axum and other paths to Next.js. Configure `APP_ORIGIN` to the HTTPS origin and `COOKIE_SECURE=true`. Set `API_INTERNAL_URL=http://api:8080` for Next server rewrites; `NEXT_PUBLIC_API_URL` is the browser-visible HTTPS origin. Do not expose `NEXCONN_APP_SECRET` or `SERPAPI_KEY` to the web container.

No deployment was performed or publicly verified. This repository does not contain host credentials, DNS permissions, or Nexconn Console credentials.
