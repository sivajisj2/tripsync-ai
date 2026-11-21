-- Add migration script here
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS trips (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    destination TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS trip_stops (
    id TEXT PRIMARY KEY,
    trip_id TEXT NOT NULL,
    place_id TEXT NOT NULL,
    name TEXT NOT NULL,
    address TEXT,
    added_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (trip_id) REFERENCES trips(id) ON DELETE CASCADE,
    UNIQUE(trip_id, place_id)
);

CREATE TABLE IF NOT EXISTS votes (
    trip_id TEXT NOT NULL,
    stop_id TEXT NOT NULL,
    voter_id TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (stop_id, voter_id),
    FOREIGN KEY (trip_id) REFERENCES trips(id) ON DELETE CASCADE,
    FOREIGN KEY (stop_id) REFERENCES trip_stops(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_stops_trip
ON trip_stops(trip_id);

CREATE INDEX IF NOT EXISTS idx_votes_trip
ON votes(trip_id);