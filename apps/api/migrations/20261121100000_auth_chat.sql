CREATE TABLE users (
    id TEXT PRIMARY KEY,
    email TEXT NOT NULL UNIQUE COLLATE NOCASE,
    display_name TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    csrf_token TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX idx_sessions_user_expiry ON sessions(user_id, expires_at);
CREATE INDEX idx_sessions_expiry ON sessions(expires_at);

CREATE TABLE trip_members (
    trip_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('owner', 'member')),
    joined_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (trip_id, user_id),
    FOREIGN KEY (trip_id) REFERENCES trips(id) ON DELETE CASCADE,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX idx_trip_members_user ON trip_members(user_id, trip_id);

CREATE TABLE nexconn_trip_groups (
    trip_id TEXT PRIMARY KEY,
    channel_id TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (trip_id) REFERENCES trips(id) ON DELETE CASCADE
);

CREATE TABLE nexconn_group_members (
    trip_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    joined_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (trip_id, user_id),
    FOREIGN KEY (trip_id) REFERENCES trips(id) ON DELETE CASCADE,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
