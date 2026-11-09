# Wayfinder Slate — M4: Save Ingestion — Implementation Plan

> Written: 2026-11-09
> Target repo: `/workspace/wayfinder-slate`
> Branch: `feat/m4-save-ingestion` created from `develop`
> Self-contained hand-off plan for a fresh build session. Decisions below are
> settled — implement them; do not re-litigate them.

## 0. Orientation (read first)

- `/workspace` is **not** the repo root. The repo lives entirely in
  `/workspace/wayfinder-slate`. Read `/workspace/INDEX.md` for orientation.
- `/workspace/maps/` (zeldadungeon/maps clone) and `/workspace/save/` (real BotW
  save sample, personal data) are **read-only reference**. Never modify, never
  commit, never redistribute the save.
- Read `README.md`, `CONTRIBUTING.md`, and `docs/PLAN.md` (esp. §4.3–4.4, §7.4,
  §7.5, §7.6, §8, §10, §11) before writing code. `docs/m3-plan.md` and
  `docs/m5-plan.md` are the established plan format and workflow model.
- Milestone to implement (`docs/PLAN.md` §8, M4 — only these boxes are open):
  - [ ] Folder watcher with debounce/hash/retry
  - [ ] Snapshot storage and retention
  - [ ] Multipart upload and path import
  - [ ] Slot selection API
  - Tick these boxes in `docs/PLAN.md` when done (do not tick them in this doc).
- Current state: M0–M3 and M5 are complete (`0.6.0` on `develop`). Save data is
  currently re-read from `WAYFINDER_SAVE_DIR` on **every request** in
  `crates/wayfinder-server/src/saves.rs`; there is no `save_slots` or
  `snapshots` table and hub mode can serve no save data at all. M4 introduces
  ingestion and makes stored snapshots the single read path, which is the
  prerequisite for M6 progress charts and M7 agent snapshot sync.

## 1. Environment facts

- Passwordless-sudo Arch Linux container. Toolchain present:
  `rustc 1.99.1`, `cargo 1.99.1`, `node 26.13.0`, `npm 12.2.2`.
- `frontend/node_modules` is already installed.
- Map assets are **not needed** for M4; they are only needed for the visual map
  smoke test (`./scripts/fetch-map-assets.sh`, ~49 MB, gitignored).
- The development save is referenced with
  `WAYFINDER_TEST_SAVE=/workspace/save/save_0/0/game_data.sav` (never committed).
- Every check in §9 must pass before the milestone is considered complete.

## 2. Settled decisions

### 2.1 Stored snapshots are the single read path

The API never parses `game_data.sav` per request again. The folder watcher (or
startup scan), upload, and import all write `save_slots` + `snapshots`; the
existing `/api/slots*` handlers read those tables. This gives local, LAN, and
hub one code path and lets M6 chart history and M7 accept agent snapshots
without further refactoring.

The only per-request filesystem access left is none; caption images are stored
as blobs. The source folder can disappear and previously stored history remains
readable.

### 2.2 Ownership model — machine-local slots vs profile slots

`save_slots.profile_id` is **nullable**:

- `profile_id IS NULL` → a **machine-local** slot, ingested from the local
  `WAYFINDER_SAVE_DIR` or from `import-path`. It is never exposed in hub mode.
- `profile_id = <profile>` → a **profile-owned** slot, created by upload (and,
  later, the M7 agent).

This avoids inventing a hidden "local user/profile" row just to satisfy the
foreign key. It also matches the real semantics: in local/LAN the save folder
belongs to the machine, not to an account.

Endpoint resolution:

- `/api/slots*` in **local/LAN** → machine-local slots (`profile_id IS NULL`).
- `/api/slots*` in **hub** → the signed-in user's **primary profile** slots
  (first profile by `created_at`, which is the auto-created `Default` profile).
- `/api/profiles/{id}/slots` → that profile's slots, always ownership-checked.

Multi-profile viewing/selection in the UI is deferred; M4 only *views* the
primary profile and lets the Saves page upload to any owned profile.

### 2.3 Schema: metadata on `save_slots`, history on `snapshots`

- `save_slots` stores identity, source, caption blobs, `raw_hash`, the **current**
  per-object completion arrays, and `present` (for local deletion detection).
- `snapshots` stores one compact row per observed save state: stats columns plus
  `stats_json` and a **count-only** `completion_summary_json`. The full
  per-object completion arrays are *not* duplicated into history; they live only
  on `save_slots.completion_json` because the map UI only needs current state.
- Exactly one snapshot is added when `game_data.sav`'s SHA-256 changes. A
  re-observation with the same hash updates caption metadata/label only.
- `present` is set to `0` for machine-local slots that disappear from the folder;
  list endpoints filter `present = 1`, preserving today's behaviour while
  keeping history. Uploads are always `present = 1`.

### 2.4 Retention and raw saves

- `WAYFINDER_SNAPSHOT_LIMIT` — max snapshots retained **per slot**, default
  `500`; `0` means unlimited. Prune oldest by `ingested_at DESC` (observation
  order, not caption time) after each insert, never deleting the row just
  inserted.
- `WAYFINDER_KEEP_RAW_SAVES` — default `false`. When `true`, store a
  gzip-compressed copy of `game_data.sav` in `snapshots.raw_blob` and its
  uncompressed size in `raw_size`. Raw blobs are never exposed by any API in v1.
  Document that enabling this with a large retention limit costs disk
  (~1 MB/save compressed).
- `game_data.sav` on disk remains the fallback for re-parsing; snapshots store
  derived data, not the raw file, by default.

### 2.5 Watcher, debounce, and retry

- Use `notify` 8.2 with `RecommendedWatcher` in recursive mode on the resolved
  save root (`.../save` or `.../save_0` style layouts already handled by
  `Config`).
- Debounce: 750 ms of quiet after the first relevant event, capped at 2 s; then
  run a full machine scan. A full scan is cheap (the sample save is ~1 MB and
  there are a handful of slots) and avoids fragile per-path mapping on Windows
  rename semantics.
- The scan hashes each `game_data.sav`; unchanged slots are skipped without
  parsing. Only changed slots are read + parsed.
- Partial writes: on `TooSmall`, `InvalidLength`, `MissingSentinel`, or an I/O
  error, retry the read+parse up to 3 times with 250/500/1000 ms backoff. On
  final failure log a warning and keep the previous good snapshot. Do **not**
  retry `UnsupportedEndianness` or `BadMarker` (definitively not a supported
  save).
- `WAYFINDER_WATCH` (default `true`) disables the watcher while leaving startup
  scan and manual import available. Hub mode never starts a watcher.
- Watcher failure (e.g. inotify limit) is logged and disables the watcher; the
  server keeps serving stored data and uploads keep working.
- Ingest operations are serialized with one `tokio::sync::Mutex<()>` to keep
  SQLite writes simple; frequency is low.

### 2.6 Upload and import semantics

- Upload stores bytes only in SQLite; no user-supplied filenames touch the
  filesystem. Part **names** are fixed: `game_data`, `caption_sav`,
  `caption_image`. `game_data` is required; the caption parts are optional.
- Validation: `game_data` must parse as a BotW save (`SaveFile::parse`); if
  `caption_sav` is present it must also parse; `caption_image` must start with
  the JPEG magic `FF D8 FF` and be ≤ 2 MiB.
- Limits: `WAYFINDER_MAX_UPLOAD_BYTES` default `8 MiB` total request;
  `game_data` ≤ 4 MiB, `caption_sav` ≤ 1 MiB, `caption_image` ≤ 2 MiB. Oversize
  → `413`; unknown parts → `400`.
- `slot_index` is optional. If omitted, allocate the next free index for that
  profile. If supplied and the slot exists, it is replaced (a new snapshot is
  added if the hash changed) and the response is `200`; otherwise `201`.
- `import-path` is **machine-scoped** and **local mode only**, because the files
  it reads are the machine's own save folder. The route is
  `POST /api/saves/import-path` rather than the profile-scoped route sketched in
  PLAN §7.5; a profile id is meaningless for machine-local ingestion. Hub/LAN
  receive `403`, and hub has no filesystem save exposure.
- Path import reads the same `{game_data.sav, caption.sav, caption.jpg}` layout
  for every numbered slot it finds, with the same retry rules. Limit to 64
  slots per request. Response reports per-slot success/failure; it is not
  all-or-nothing.

### 2.7 API compatibility

- The existing `slot` integer in `/api/slots*` remains the **slot index** (the
  numeric folder name for machine slots; the per-profile index for uploads), so
  `App.svelte`, `Dashboard.svelte`, `MapView.svelte`, and their tests keep
  working with minimal changes.
- `SlotSummary`, `SlotDetail`, `MapCompletion`, and `CompletionSummary` JSON
  shapes stay unchanged. Only their backing store changes.
- Hub mode finally shows data: `/api/slots` becomes available to signed-in hub
  users (primary profile). `App.svelte` must stop short-circuiting hub mode.

### 2.8 Frontend scope

Minimal and functional:

- `App.svelte`: allow slot loading in hub mode when a user is signed in; show
  the slot picker whenever slots exist; reload after login/upload.
- New `frontend/src/pages/Saves.svelte` + nav entry:
  - signed-in/hub: list owned profiles, list that profile's slots, upload a
    slot (`game_data`, optional `caption.sav`/`caption.jpg`, optional slot
    number);
  - local mode: show the configured save folder and an `import-path` form.
- No new npm packages, no drag-and-drop library, plain `<input type="file">`.

## 3. Dependencies to add

Workspace `Cargo.toml` (`[workspace.dependencies]`):

```toml
notify = "8.2"        # 8.2.2 is the current stable at 2026-11-09
sha2 = "0.11"         # SHA-256 for change detection
hex = "0.4"           # lowercase hex hashes
flate2 = "1.1"        # gzip for optional raw save retention
```

`crates/wayfinder-server/Cargo.toml`:

```toml
axum = { workspace = true, features = ["multipart"] }
flate2.workspace = true
hex.workspace = true
notify.workspace = true
sha2.workspace = true
```

Notes:

- `flate2`, `hex`, and `sha2` are already in `Cargo.lock` transitively; adding
  them directly does not add a second copy.
- `tokio` already has `sync`, `fs`, `time`, and `rt-multi-thread`; no new tokio
  features are needed.
- Verify `sha2` 0.11's `Digest` import at code time (`sha2::{Digest, Sha256}`).
- Do not add `walkdir`: `std::fs::read_dir` over one level is enough for the
  known layout.

## 4. Work breakdown

### 4.1 Migration `migrations/00000000000002_m4_ingest.sql`

Use runtime queries only (M3 style), no `query!` macros.

```sql
CREATE TABLE save_slots (
    id               TEXT PRIMARY KEY,              -- UUIDv7 as UUID string
    profile_id       TEXT REFERENCES profiles(id) ON DELETE CASCADE,
    slot_index       INTEGER NOT NULL,
    label            TEXT NOT NULL,
    caption_location TEXT NOT NULL DEFAULT '',
    caption_map_area TEXT NOT NULL DEFAULT '',
    source           TEXT NOT NULL CHECK (source IN ('local', 'upload', 'agent')),
    last_modified    INTEGER NOT NULL,              -- unix seconds
    raw_hash         TEXT NOT NULL,                 -- sha256 hex of game_data.sav
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
    captured_at             INTEGER NOT NULL,       -- caption saved_at when sane, else mtime/now
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
```

SQLite `ON CONFLICT` inference with partial unique indexes is error-prone; use
a `SELECT ... FOR UPDATE`-style read inside a transaction followed by
`INSERT`/`UPDATE` rather than `INSERT ... ON CONFLICT` on those indexes.

### 4.2 Config and discovery (`config.rs`)

New fields + env vars:

| Field | Env | Default |
| --- | --- | --- |
| `snapshot_limit` | `WAYFINDER_SNAPSHOT_LIMIT` | `500` per slot |
| `keep_raw_saves` | `WAYFINDER_KEEP_RAW_SAVES` | `false` |
| `watch_saves` | `WAYFINDER_WATCH` | `true` |
| `max_upload_bytes` | `WAYFINDER_MAX_UPLOAD_BYTES` | `8 * 1024 * 1024` |

- Reuse the `parse_cookie_secure`-style boolean parsing (factor a
  `parse_bool` helper rather than duplicating).
- Parse the two numeric values with clear errors; clamp
  `max_upload_bytes` to a sane minimum (e.g. 512 KiB) so `game_data.sav` always
  fits.
- Refactor slot discovery into a path-parameterized
  `pub struct DiscoveredSlot { pub index: usize, pub dir: PathBuf }` and
  `pub fn discover_slots(root: &Path) -> Vec<DiscoveredSlot>` supporting the
  three layouts already handled (save root with `save_0`, profile folder, direct
  slot folder). Keep `Config::save_slots()` / `save_slot_path()` as thin
  wrappers for tests and the watcher, or update callers.
- Add `Config::save_root()` returning the directory the watcher should watch
  (the profile folder when a `save_0` root is configured, otherwise the save
  root itself).
- Update every explicit `Config { ... }` test constructor (config.rs, http.rs,
  and any new tests) for the four new fields.

### 4.3 Ingest module (`src/ingest/mod.rs`)

Shared by startup scan, watcher, upload, and import. Keep HTTP out of it.

```rust
pub enum IngestSource { Local, Upload, Agent }

pub struct IngestTarget {
    pub profile_id: Option<String>,   // None = machine-local
    pub slot_index: u32,
    pub source: IngestSource,
}

pub struct SavePayload {
    pub game_data: Vec<u8>,
    pub caption_sav: Option<Vec<u8>>,
    pub caption_image: Option<Vec<u8>>,
    pub last_modified: i64,
}

pub enum IngestOutcome { Created, Updated, Unchanged }

pub async fn ingest(
    pool: &SqlitePool,
    objects: &MapObjects,
    target: IngestTarget,
    payload: SavePayload,
    options: &IngestOptions,
) -> Result<IngestOutcome, IngestError>;
```

Flow:

1. Compute `raw_hash = hex(Sha256(game_data))`. If a slot with the same target
   exists and its `raw_hash` matches, update caption/label/presence/timestamps
   only and return `Unchanged`.
2. `spawn_blocking` the parse (`SaveFile::parse`) and all derivations:
   `stats()`, `caption()`, `completion_summary()`, and the current
   `CompletedObjects` (make `saves::completed_objects(&SaveFile, &MapObjects)`
   `pub(crate)` and reuse it — do not duplicate the flag loops).
3. Validate the header version; `game_version` is `"1.9.0"` for
   `VERSION_1_9_0`, otherwise `format!("0x{:08x}", version)`. Do not reject
   structurally valid saves from other little-endian Switch versions.
4. Open one transaction:
   - resolve/insert the slot row (`id = Uuid::now_v7()` for new rows);
   - insert the snapshot row;
   - prune old snapshots if `snapshot_limit > 0`, ordered by `ingested_at DESC`,
     protecting the new row;
   - update `save_slots` (`raw_hash`, `completion_json`, caption blobs, label,
     `last_modified`, `updated_at`, `present = 1`).
5. gzip the raw save only when `keep_raw_saves`; cap at the same 4 MiB.
6. Return `Created`/`Updated`; the HTTP layer reloads the `SlotSummary` for the
   response.

Helpers:

- `fn game_version_label(version: u32) -> String`
- `fn ingest_options(config: &Config) -> IngestOptions`
- `fn label_for(caption: &SaveCaption, slot_index: u32, objects: &MapObjects) -> String`
  (same logic as today's `summary_for`).
- `captured_at`: use `caption.saved_at` when it is > 0 and not more than one day
  in the future; otherwise `last_modified` when sane, else now.

### 4.4 Startup scan and watcher (`src/ingest/watcher.rs`, `src/main.rs`)

`pub async fn scan_machine_slots(config, pool, objects) -> ScanReport`:

- Resolve `save_root`; no-op in hub mode or when no folder is configured.
- `discover_slots`; for each slot, read files with retry (§2.5), call `ingest`
  with `profile_id = None`, `source = Local`, `last_modified` = file mtime.
- After the scan, `UPDATE save_slots SET present = 0 WHERE profile_id IS NULL AND
  present = 1 AND slot_index NOT IN (...)`; log counts only (no paths/labels in
  info logs).
- Errors are per-slot and logged at warn; the scan continues.

Startup (`main.rs`):

1. Open DB/migrations (existing).
2. Call `scan_machine_slots` **before** building the router so the first
   `/api/slots` request has data. Never make startup fatal on scan failure.
3. If `watch_saves && mode != Hub && save_root.is_some()`, spawn the watcher and
   keep a handle.
4. Serve as today; after `axum::serve` returns, signal and await watcher
   shutdown (a `tokio::sync::watch` channel is enough; do not add `tokio-util`).

Watcher loop:

- `notify::recommended_watcher` with a callback that forwards events into a
  `tokio::sync::mpsc` channel (`blocking_send` from the notify thread).
- Ignore events for irrelevant paths only after checking the file name
  (`game_data.sav`, `caption.sav`, `caption.jpg`); otherwise run a full scan.
- Debounce state machine kept in a small pure helper with unit tests:
  first event starts a 750 ms quiet timer, further events reset it, total wait
  capped at 2 s.
- Wrap the scan in the shared ingest mutex.

Provide a `SaveWatcher` struct with `shutdown(self).await` (send + join) so tests
and `main` can stop cleanly.

### 4.5 Read path (`src/saves.rs`, `src/http.rs`)

`save_slots` and `snapshots` become the source of truth:

- `list_machine_slots(pool, objects)` — `profile_id IS NULL AND present = 1`,
  ordered by `slot_index`; map rows to `SlotSummary` (label, location,
  map_area, saved_at from latest snapshot).
- `list_profile_slots(pool, profile_id, objects)` — same for a profile.
- `load_machine_slot(pool, slot_index, objects)` → `SlotDetail` using the latest
  snapshot (`ORDER BY captured_at DESC, ingested_at DESC LIMIT 1`); parse
  `stats_json` and `completion_summary_json`.
- `load_profile_slot(...)` for future/primary-profile hub detail.
- `load_map_completion` reads/parses `slot.completion_json`; a parse failure
  logs a warning and returns empty arrays rather than a 500.
- `load_caption` reads `caption_image`; `CaptionNotFound` maps to `404` as
  today.
- Keep `SlotError` variants and `slot_error_response` semantics (`404`/`422`).
  `422` remains for an unreadable stored snapshot/JSON, even though ingest
  rejects bad saves earlier.

Handlers:

- `list_slots`, `slot_detail`, `slot_caption`, `slot_map`, `completion` gain the
  `AuthSession` extractor where needed to resolve hub scoping.
- Add `resolve_machine_or_primary_profile(...)`: local/LAN → machine slots; hub
  → `auth_session.user`'s primary profile (query
  `profiles WHERE user_id = ? ORDER BY created_at, id LIMIT 1`). Return `401` if
  the guard somehow let an anonymous request through, `404` if no profile.
- Add `GET /api/profiles/{id}/slots` and
  `GET /api/profiles/{id}/slots/{slot}/caption` — both auth-required,
  ownership-checked, `404` for another user's profile.
- Do not change the JSON field names in `SlotSummary`/`SlotDetail`/
  `MapCompletion`.

Refactor the old `saves.rs` unit tests to seed via `ingest()` and query the new
read path; keep the synthetic-save helpers in `test_support`.

### 4.6 Upload endpoint (`src/http.rs`)

`POST /api/profiles/{id}/saves/upload`:

- Auth required; ownership check (`profile.user_id == auth user.id`), `404` for
  missing/foreign profiles (do not leak existence).
- `Multipart` extractor; apply `DefaultBodyLimit::max(config.max_upload_bytes)`
  on this route.
- Read parts by **name** (`game_data`, `caption_sav`, `caption_image`); ignore
  filenames; reject duplicate names and unknown names with `400`.
- Validate sizes and magic/parseability per §2.6; map invalid save to `422`,
  oversize to `413`, malformed multipart to `400`.
- Resolve `slot_index` (given or next free) inside the ingest transaction;
  `slot_index` must fit `u32` and (for sanity) `0..=255`.
- Ingest with `source = Upload`, `profile_id = Some(id)`, `last_modified = now`.
- Respond `201` on create / `200` on replace with the resulting `SlotSummary`.
- No filesystem writes.

### 4.7 Path import endpoint (`src/http.rs`)

`POST /api/saves/import-path`:

- `403` unless `mode == Local` (per §2.6).
- Body `{ "path": "..." }`; canonicalize, require a directory, discover slots.
- Limit 64 slots. For each: read with retry, ingest machine-local
  (`profile_id = None`, `source = Local`), collect per-slot errors.
- Response `200 { "imported": [SlotSummary], "errors": [{ "slot": 3, "error": "..." }] }`.
- `400` for missing/invalid/non-directory path; never echo the full path in
  logs at info level.

### 4.8 Frontend (`frontend/src/lib/api.ts`, `App.svelte`, `Saves.svelte`)

API additions:

- `uploadSlot(profileId, files, slotIndex?)` using `FormData`; part names match
  §4.6.
- `importPath(path)` → `POST /api/saves/import-path`.
- `getProfileSlots(profileId)`.

`App.svelte`:

- Remove the `config.mode === 'hub'` early return in `loadSlots()`; instead skip
  only when `config.auth_required && user === null`.
- `showSlotPicker` becomes `slots.length > 0` (drop `mode !== 'hub'`).
- After `handleAuthenticated`, reload slots (already done); after an upload,
  re-run `loadSlots()`.
- Add a `saves` page id/nav entry (`Saves`, icon e.g. `▣`).

`Saves.svelte`:

- Signed in: fetch `getProfiles()`, select profile, show `getProfileSlots()`,
  and an upload card. Files: `game_data` required, `caption_sav` and
  `caption_image` optional, optional slot number. Show per-request errors from
  `ApiError`.
- Local mode: show `config.save_dir` and an import-path form; success lists
  imported slot labels.
- Mobile-first, consistent with `app.css`; no new dependencies.

Hub slot loading depends on §4.5's primary-profile resolution; make sure the
post-login reload happens after the auth cookie is set.

### 4.9 Tests

Rust (`wayfinder-server`):

1. Migration: `save_slots`/`snapshots` and all indexes exist after
   `open_pool`; running twice is a no-op.
2. Ingest: synthetic save + caption creates a slot and one snapshot with the
   expected stats; re-ingest with the same bytes is `Unchanged` and adds no
   snapshot; changed bytes add a second snapshot; the map completion JSON
   matches the flags in the synthetic save.
3. Retention: with `snapshot_limit = 2`, three distinct saves leave two
   snapshots (the two newest by `ingested_at`); limit `0` keeps all.
4. Raw saves: default leaves `raw_blob` NULL; with `keep_raw_saves = true` it is
   a gzip payload (decompress in the test and compare).
5. Presence: deleting a machine slot directory and rescanning sets
   `present = 0` and removes it from `/api/slots` while snapshots remain.
6. Read path: `/api/slots`, detail, map, and caption after ingest; unknown slot
   → 404; stored detail JSON field names unchanged from the M5 tests.
7. Hub scoping: anonymous `/api/slots` → 401; with login → primary profile
   slots; a slot uploaded to a second profile is not visible via `/api/slots`.
8. Upload: happy path (with and without caption files), replace, next-free slot
   allocation, invalid save → 422, wrong magic for caption image → 422/400,
   unknown part → 400, owner mismatch/foreign profile → 404, oversize → 413.
9. Import path: local mode imports a temp folder's slots; LAN/hub → 403;
   missing path → 400; per-slot error is reported without aborting the good
   slots.
10. Debounce helper unit tests (quiet reset and 2 s cap). Do **not** write a
    flaky end-to-end `notify` test; exercise `scan_machine_slots` directly and
    cover the watcher manually in the smoke test.
11. Existing suites (config, auth, http policy, save parser) stay green.

Real-save check (private, not committed):

```sh
WAYFINDER_TEST_SAVE=/workspace/save/save_0/0/game_data.sav \
  cargo test -p wayfinder-save --test real_save
```

Add one gated server integration test that, when `WAYFINDER_TEST_SAVE` points at
a profile/slot, ingests it and asserts the parsed stats/completion still match
the parser's own results.

Frontend: `svelte-check` must pass; add unit tests only if a pure helper is
introduced. No new vitest dependencies.

## 5. API contract summary

| Endpoint | Method | Scope policy | Response |
| --- | --- | --- | --- |
| `/api/slots` | GET | local open; LAN auth; hub auth | `200 [SlotSummary]` (machine or primary-profile slots) |
| `/api/slots/{slot}` | GET | same as above | `200 SlotDetail` / `404` / `422` |
| `/api/slots/{slot}/caption` | GET | same as above | `200 image/jpeg` / `404` |
| `/api/slots/{slot}/map` | GET | same as above | `200 MapCompletion` / `404` / `422` |
| `/api/completion?slot=N` | GET | same as above | `200 CompletionSummary` / `404` / `422` |
| `/api/profiles/{id}/slots` | GET | auth + ownership | `200 [SlotSummary]` / `404` |
| `/api/profiles/{id}/slots/{slot}/caption` | GET | auth + ownership | `200 image/jpeg` / `404` |
| `/api/profiles/{id}/saves/upload` | POST | auth + ownership | `201`/`200 SlotSummary` / `400` / `404` / `413` / `422` |
| `/api/saves/import-path` | POST | local mode only | `200 {imported, errors}` / `400` / `403` |

`SlotSummary` and `SlotDetail` keep their existing shapes. `slot` remains the
integer slot index.

## 6. Documentation

- `README.md`: add the four new env vars to the configuration table; add a short
  "Saved data and snapshots" section covering the watcher, retention, raw-save
  opt-in, uploads in hub mode, and `import-path` being local-only.
- `CHANGELOG.md` `[Unreleased]` → `### Added` (ingestion schema, startup scan,
  watcher, snapshots/retention, upload/import APIs, Saves page) and `### Changed`
  (save APIs now read stored snapshots; hub mode shows uploaded saves). Do not
  bump the version in the feature commits; cut `0.7.0` when the milestone lands.
- `docs/PLAN.md`: tick the four M4 boxes. Add a short note under §7.4/§7.5 about
  the machine-local `profile_id IS NULL` source and the machine-scoped
  `import-path` route (deliberate refinement of the sketch).
- `CONTRIBUTING.md`: no check changes (existing fmt/clippy/test/check/build
  commands cover M4).
- `THIRD_PARTY_NOTICES.md`: no change (no new assets).

## 7. Commit plan (conventional commits, focused)

1. `feat(server): add save ingestion schema and snapshot store`
2. `feat(server): ingest save folders on startup`
3. `feat(server): watch the save folder with debounce and retry`
4. `feat(server): serve save slots from stored snapshots`
5. `feat(server): add save upload and path import APIs`
6. `feat(frontend): add save upload and import page`
7. `test(server): cover ingestion, retention, upload, and import`
8. `docs: document M4 save ingestion`

Create the branch first:

```sh
git switch develop
git pull
git switch -c feat/m4-save-ingestion
```

Do all work on the feature branch, not `develop`.

## 8. Verification checklist (must all be green)

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

npm --prefix frontend install
npm --prefix frontend run check
npm --prefix frontend run test
npm --prefix frontend run build

node --check scripts/fetch-map-assets.mjs
node --check scripts/build-treasure-objects.mjs
node --check scripts/build-divine-beasts.mjs
node --check scripts/validate-map-objects.mjs

./scripts/fetch-map-assets.sh
node scripts/validate-map-objects.mjs
```

Manual smoke test (use a scratch data dir):

```sh
npm --prefix frontend run build
WAYFINDER_MODE=local \
WAYFINDER_DATA_DIR=$(mktemp -d) \
WAYFINDER_SAVE_DIR=/workspace/save \
  cargo run -p wayfinder-server
# 1. /api/slots lists the sample slots from stored snapshots (not per-request disk reads)
# 2. touch/rewrite a slot's game_data.sav -> watcher adds exactly one snapshot
# 3. write a truncated file -> warning, previous snapshot still served
# 4. POST /api/saves/import-path with a copy of the save root -> slots import
# 5. sqlite3 data/wayfinder.sqlite3 'select count(*) from snapshots;' increments
```

Hub smoke test:

```sh
WAYFINDER_MODE=hub \
WAYFINDER_DATA_DIR=$(mktemp -d) \
WAYFINDER_BIND=127.0.0.1:18080 \
  cargo run -p wayfinder-server
# register/login, open Saves, upload game_data.sav (+ optional captions),
# then Dashboard/Map show the uploaded slot.
```

Also confirm the real save is never copied into the repo (`git status` clean of
save files) and that `data/` remains gitignored.

## 9. Deferred / out of scope (do not sneak in)

- M6: progress-over-time charts, medals/titles, global opt-in aggregates,
  privacy controls. (M4 stores the data they need; it does not render it.)
- M7: live telemetry, SSE/WebSocket, `wayfinder-control` polling, the agent push
  binary, agent tokens, `live_sessions`, agent snapshot upload. The ingest
  module must be reusable by the agent later, but do not add the endpoints.
- M8: rate limiting, origin/CSRF hardening, upload malware scanning, fuzz
  suite, Docker/reverse-proxy docs, release build.
- Multi-profile UI/selection, profile rename/delete, public profiles.
- Zip/folder-upload multipart formats; only the three named parts above.
- Serving or downloading raw save blobs; they are private internal data.
- `option.sav` and `tracker/trackblock*.sav` parsing.
- Automatic re-parse of historical raw snapshots after schema updates.

## 10. Pitfalls to verify at code time

- **`notify` API**: use `RecommendedWatcher::new(handler, notify::Config::default())`
  and `watch(root, RecursiveMode::Recursive)`; the handler runs on a non-async
  thread, so use `blocking_send` on a tokio mpsc channel. Do not `.unwrap()`
  watcher setup; log and disable on failure.
- **Debounce on Windows**: rename/replace events are noisy; always rescan the
  whole root after the quiet period and compare hashes. Never trust a single
  event path.
- **Partial writes**: retry only structural/I/O failures. A `BadMarker` or
  `UnsupportedEndianness` is a real error, not a race.
- **SQLite partial unique indexes**: `ON CONFLICT` target inference is fragile;
  do the SELECT-then-INSERT/UPDATE inside a transaction.
- **NULL semantics**: queries for machine slots must use `profile_id IS NULL`,
  never `= NULL`.
- **Retention ordering**: prune by `ingested_at`, not `captured_at`, and exclude
  the just-inserted snapshot id. Caption timestamps can move backwards after an
  older save is uploaded.
- **Snapshot dedupe**: hash the exact bytes before parsing; the game can rewrite
  identical content. Same hash should not grow history.
- **Presence**: a failed scan must not mark slots absent. Only a successful
  directory enumeration may set `present = 0`; keep the previous state on I/O
  errors.
- **`spawn_blocking`**: parse and hash off the async runtime per
  `CONTRIBUTING.md`. File reads may use `tokio::fs`.
- **Memory**: `SaveFile::parse` retains the raw bytes; drop the parsed value
  after ingest and do not hold the payload across `.await` points longer than
  necessary.
- **Uploads**: never use user filenames for storage; key by multipart part
  name. Sniff the JPEG magic, do not trust `Content-Type`. Apply the body limit
  on the route, not just globally.
- **Auth scoping**: hub `/api/slots*` must resolve the caller's primary profile;
  ownership failures return `404` (not `403`) so profile ids cannot be
  enumerated. Local/LAN keep machine slots.
- **Frontend auth timing**: reload slots after the login response has set the
  cookie; do not fetch `/api/slots` as an anonymous hub user.
- **JSON shape stability**: `SlotSummary`/`SlotDetail`/`MapCompletion` are
  consumed by M5 tests and components; changing field names breaks the map and
  dashboard.
- **No PII in logs**: do not log save paths, player positions, labels, or raw
  hashes at info level.
- **Timezone**: all timestamps are unix seconds (`i64`); caption `saved_at` is a
  `u32` in the save — widen before storing and sanity-check it.