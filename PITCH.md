# Hackathon Pitch

## Problem

Planning a group trip is a coordination problem: inspiration is easy to collect, but hard to turn into a shared, trusted decision.

## Solution

TripSync turns real destination search results into a collaborative itinerary. Groups can share a trip, add places, vote, tune category preferences, and review a smart route before applying it.

## Why These Integrations Matter

- **SerpApi:** supplies live Google Maps place discovery instead of fabricated recommendations.
- **Nexconn:** provides provider-hosted group chat, message delivery, and history tied to each trip (implementation is present; live provider behavior awaits valid credentials and verification).
- **Rust WebSockets:** deliver trip-isolated stop/vote updates and reconnect synchronization.
- **Smart ranking:** makes its rating, group preference, vote, and coordinate-availability weights visible and deterministic.

## Technical Differentiation

Axum/Tokio, SQLite WAL, SQLx migrations, unique vote constraints, transactional itinerary updates, Argon2 authentication, server-side sessions, CSRF checks, and explicit shared-trip membership keep the demo coherent from discovery through group decisions.

## Scalability Path

The current single-node SQLite/WAL setup is sized for hackathon and small-group demos. A multi-instance product should move to a managed relational database, durable metrics/log aggregation, distributed rate limits, and a shared WebSocket/pub-sub layer. Keep Nexconn and SerpApi keys server-side and instrument provider failures before scaling.

## Verified Scope

Rust and frontend checks are run from the repo commands. Live Nexconn message exchange, manual two-browser UI testing, public hosting, and load benchmarks require credentials or infrastructure not present in this workspace and must not be presented as completed until verified.
