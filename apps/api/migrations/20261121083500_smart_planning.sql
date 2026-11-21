ALTER TABLE trip_stops ADD COLUMN latitude REAL;
ALTER TABLE trip_stops ADD COLUMN longitude REAL;
ALTER TABLE trip_stops ADD COLUMN rating REAL;
ALTER TABLE trip_stops ADD COLUMN category TEXT;
ALTER TABLE trip_stops ADD COLUMN itinerary_position INTEGER;

CREATE INDEX idx_trip_stops_trip_position
ON trip_stops(trip_id, itinerary_position, added_at);

CREATE TABLE trip_preferences (
    trip_id TEXT NOT NULL,
    category TEXT NOT NULL,
    weight INTEGER NOT NULL CHECK (weight BETWEEN 0 AND 5),
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (trip_id, category),
    FOREIGN KEY (trip_id) REFERENCES trips(id) ON DELETE CASCADE
);

CREATE TABLE itinerary_plans (
    id TEXT PRIMARY KEY,
    trip_id TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (trip_id) REFERENCES trips(id) ON DELETE CASCADE
);

CREATE INDEX idx_itinerary_plans_trip_created
ON itinerary_plans(trip_id, created_at DESC);

CREATE TABLE itinerary_plan_stops (
    plan_id TEXT NOT NULL,
    stop_id TEXT NOT NULL,
    position INTEGER NOT NULL CHECK (position >= 0),
    score REAL NOT NULL,
    explanation TEXT NOT NULL,
    PRIMARY KEY (plan_id, stop_id),
    UNIQUE (plan_id, position),
    FOREIGN KEY (plan_id) REFERENCES itinerary_plans(id) ON DELETE CASCADE,
    FOREIGN KEY (stop_id) REFERENCES trip_stops(id) ON DELETE CASCADE
);

CREATE INDEX idx_itinerary_plan_stops_order
ON itinerary_plan_stops(plan_id, position);
