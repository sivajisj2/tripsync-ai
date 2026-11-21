# Hackathon Submission Checklist

- [x] Real SerpApi integration and documented environment variable.
- [x] Trip sharing, membership model, authenticated votes, and duplicate protection.
- [x] Rust trip-scoped WebSockets and reconnect sync event.
- [x] Transparent deterministic recommendation formula and persisted reviewed plans.
- [x] Argon2 passwords, HttpOnly sessions, CSRF/origin checks, expiry, and auth rate limits.
- [x] Nexconn official SDK/API integration paths, with App Secret server-only.
- [x] Dockerfiles, Compose, TLS proxy, and persistent SQLite volume configuration.
- [ ] Configure valid `NEXCONN_APP_KEY` and `NEXCONN_APP_SECRET`; verify two-user messaging/history/reconnect.
- [ ] Configure real `SERPAPI_KEY` in the target demo environment.
- [ ] Run manual two-browser end-to-end workflow and capture screenshots.
- [ ] Install k6 and save a real benchmark report, if required by judging.
- [ ] Provision a host with persistent storage, HTTPS/WSS, DNS, and inspect billing before deployment.
- [ ] Verify public deployment URLs before claiming deployment.
- [ ] Review staged files and create feature commits; do not include `.env` or provider secrets.
