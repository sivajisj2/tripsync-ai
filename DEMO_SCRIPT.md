# Three-Minute Demo Script

## 0:00–0:25 — The problem

“Group trips scatter discovery, preferences, and decisions across tabs and group chats. TripSync gives the group one shared place to decide what is worth doing.”

## 0:25–1:05 — Real discovery

Search a destination and explain that SerpApi returns actual Google Maps places. Show ratings and place links; emphasize that unavailable signals are labeled rather than guessed.

## 1:05–1:45 — Shared plan

Create a named trip, add two or three places, and show live votes. Generate the transparent scored draft, move a stop, then explicitly confirm applying it.

## 1:45–2:30 — Collaboration

Open the share link in a second browser, sign in as another user, join, and vote. Show the Rust trip-scoped WebSocket update and refresh persistence. If Nexconn has been verified with credentials, send a real message each direction and show provider history.

## 2:30–3:00 — Technical distinction

“Rust/Axum and SQLite transactions protect membership and concurrent votes; WebSockets sync collaborators; SerpApi supplies real places; a deterministic weighted score plus nearest-neighbor ordering helps the group choose. Nexconn provides actual group messaging when configured.”

Do not claim Nexconn messaging or deployment is live until both have been tested with real credentials and URLs.
