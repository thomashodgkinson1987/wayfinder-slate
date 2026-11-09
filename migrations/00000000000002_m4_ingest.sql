-- M4: save ingestion. Stored snapshots become the API's source of truth.
--
-- profile_id IS NULL marks a machine-local slot (the local/LAN save folder).
-- A non-NULL profile_id marks a profile-owned slot (upload, later the agent).

CREATE TABLE save_slots (
    id               TEXT PRIMARY KEY,              -- UUIDv7 as UUID string
    profile_id       TEXT REFERENCES profiles(id) ON DELETE CASCADE,
    slot_index       INTEGER NOT NULL,
    label            TEXT NOT NULL,
    caption_location TEXT NOT NULL DEFAULT '',
    caption_map_area TEXT NOT NULL DEFAULT '',
    source           TEXT NOT NULL CHECK (source IN ('local', 'upload', 'agent')),
    last_modified    INTEGER NOT NULL,              -- unix seconds
    raw_hash         TEXT NOT NULL,                 -- SHA-256 hex of game_data.sav
    caption_sav      BLOB,
    caption_image    BLOB,
    completion_json  TEXT NOT NULL DEFAULT '{}',    -- current per-object arrays
    present          INTEGER NOT NULL DEFAULT 1,    -- SQLite bool
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL
);

-- NULL profile_id means the single machine-local source.
CREATE UNIQUE INDEX idx_save_slots_machine
    ON save_slots (slot_index) WHERE profile_id IS NULL;
CREATE UNIQUE INDEX idx_save_slots_profile
    ON save_slots (profile_id, slot_index) WHERE profile_id IS NOT NULL;
CREATE INDEX idx_save_slots_profile_created
    ON save_slots (profile_id, created_at);

CREATE TABLE snapshots (
    id                      TEXT PRIMARY KEY,       -- UUIDv7
    slot_id                 TEXT NOT NULL REFERENCES save_slots(id) ON DELETE CASCADE,
    captured_at             INTEGER NOT NULL,       -- caption time, else mtime, else now
    ingested_at             INTEGER NOT NULL,       -- wall clock at ingestion
    game_version            TEXT NOT NULL,          -- "1.9.0" or "0x00004730"
    playtime_s              INTEGER NOT NULL,
    rupees                  INTEGER NOT NULL,
    hearts_current          REAL NOT NULL,
    hearts_max              REAL NOT NULL,
    stamina_current         REAL NOT NULL,
    stamina_max             REAL NOT NULL,
    korok_count             INTEGER NOT NULL,
    position_x              REAL NOT NULL,
    position_y              REAL NOT NULL,
    position_z              REAL NOT NULL,
    map_name                TEXT NOT NULL,
    map_type                TEXT NOT NULL,
    stats_json              TEXT NOT NULL,          -- full wayfinder_save::SaveStats
    completion_summary_json TEXT NOT NULL,          -- counts only
    raw_blob                BLOB,                   -- optional gzip, never served
    raw_size                INTEGER,
    created_at              INTEGER NOT NULL
);

CREATE INDEX idx_snapshots_slot_captured
    ON snapshots (slot_id, captured_at DESC, ingested_at DESC);
CREATE INDEX idx_snapshots_slot_ingested
    ON snapshots (slot_id, ingested_at DESC);