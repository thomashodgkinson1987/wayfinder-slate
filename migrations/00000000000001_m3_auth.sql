-- M3: users, profiles, and sessions.
-- Later milestones add save_slots, snapshots, agent_tokens, live_sessions,
-- and global_stats in their own migrations.
CREATE TABLE users (
    id TEXT PRIMARY KEY,             -- UUIDv7 as UUID string
    username TEXT NOT NULL,
    password_hash TEXT NOT NULL,     -- PHC string (Argon2id)
    display_name TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'user' CHECK (role IN ('admin', 'user')),
    created_at INTEGER NOT NULL      -- unix seconds
);

-- Case-insensitive unique usernames; queries must compare via LOWER(username).
CREATE UNIQUE INDEX users_username_unique ON users (LOWER(username));

CREATE TABLE profiles (
    id TEXT PRIMARY KEY,             -- UUIDv7 as UUID string
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    game_mode TEXT NOT NULL DEFAULT 'normal' CHECK (game_mode IN ('normal', 'master')),
    is_public INTEGER NOT NULL DEFAULT 0,
    share_global INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);

CREATE INDEX idx_profiles_user ON profiles (user_id);

-- tower-sessions records; `data` is a MessagePack blob of the session Record.
CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    data BLOB NOT NULL,
    expiry INTEGER NOT NULL          -- unix seconds
);

CREATE INDEX idx_sessions_expiry ON sessions (expiry);
