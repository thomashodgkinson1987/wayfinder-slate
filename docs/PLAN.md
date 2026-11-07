# Wayfinder Slate — Project Plan

> Status: Draft for implementation  
> Last updated: 2026-11-07  
> Project root: `wayfinder-slate/`

## 1. Vision

Wayfinder Slate is a self-hosted companion web app for **Savage Wind**, the native
recompilation of *The Legend of Zelda: Breath of the Wild* (Nintendo Switch, 1.9.0)
for PC.

It recreates the useful BotW features of Nintendo's **Zelda Notes** from local save
data and, optionally, live telemetry from Savage Wind's control server. It does not
integrate with Nintendo accounts or Nintendo Switch Online.

The app has three deployment modes:

- **Local** — runs on the gaming PC, reads the Savage Wind save folder directly, and
  can poll Savage Wind's control server for live position/stamina.
- **LAN** — the same server bound to the LAN, authenticated, usable from a phone or
  tablet as a second screen.
- **Public hub** — a remote, multi-user deployment behind a reverse proxy. Users
  register, upload or sync saves, keep their data private by default, and may opt in
  to global statistics. A small local agent can push live telemetry and save
  snapshots from the player's PC to the hub.

## 2. Goals and non-goals

### Goals

- Read BotW Switch 1.9.0 save folders produced by Savage Wind.
- Parse the save into typed, queryable data: stats, progress flags, inventory,
  position, map state, and completion.
- Present a mobile-friendly Leaflet map of Hyrule with shrines, Koroks, towers,
  memories, Divine Beasts, and completion state.
- Present play data, medals/titles, and opt-in global comparisons.
- Support local/LAN use with optional live position and stamina from Savage Wind.
- Support a public hub with accounts, private saves, and an outbound-push agent.
- Ship as a single Rust binary with the Vite frontend embedded via `rust-embed`.
- Use SQLite via `sqlx`, with embedded migrations.
- Use `axum-login` and Argon2id for authentication.

### Non-goals for v1

- Modifying save files or injecting items into the game.
- Voice Memories audio playback.
- amiibo effects or scan tracking.
- Daily Bonus in-game effects.
- Item Sharing and Autobuild Sharing.
- Photo Studio.
- Nintendo account/console integration.
- Wii U save support (Switch little-endian only for now).
- Redistributing game files or extracted game assets other than the attributed map
  tiles described below.

## 3. What Zelda Notes does for BotW

Both sources describe the BotW/TotK feature set; Autobuild Sharing is TotK-only.
Wayfinder Slate keeps the BotW-relevant features that can be reproduced from save
data. The official service is console-linked; Wayfinder Slate is save-file-driven.

| Feature | Zelda Notes behaviour | Wayfinder Slate v1 |
| --- | --- | --- |
| Navigation | Voice-guided directions to Koroks and hard-to-reach shrines; completion rates | Leaflet map with layers, completion filters, nearest-unfinished suggestions, and optional live position |
| Voice Memories | Location-triggered voice commentary/stories | Out of scope (copyrighted audio); optional text/lore notes later |
| My Play Data | Play history, stats, medals/titles | Dashboard, derived medals/titles, progress-over-time snapshots |
| Global Play Data | Aggregate worldwide player stats | Opt-in aggregate stats and leaderboards across hub users |
| Daily Bonus | Daily random in-game effect | Out of scope for v1; cannot affect the game without integration |
| amiibo | Scan tracking and bonus daily scan | Out of scope for v1 |
| Photo Studio | Edit/stamp in-game photos and share via QR | Out of scope for v1; album browsing may come later |
| Item Sharing | Deposit/withdraw items via QR item box | Out of scope for v1; would require save writing or game integration |
| Autobuild Sharing | TotK Ultrahand blueprints | Excluded — TotK only |

## 4. Save file findings

### 4.1 Verified sample

The supplied save at `/workspace/save/save_0` is a Switch 1.9.0 save:

- Header version: `0x4730`
- File size: `1,027,248` bytes
- Header: 12 bytes
- Records: 8 bytes each (`u32` CRC32 key ID, 4-byte value)
- Terminator: final 4 bytes are `0xFFFFFFFF`
- 128,404 records, 43,672 unique key IDs, 1,919 non-zero entries in slot 0
- Little-endian (Switch); Wii U saves are big-endian and are not supported initially

The folder layout observed:

```
save/
  save_0/
    0/ caption.jpg, caption.sav, game_data.sav
    1/ ... 5/
    option.sav
    tracker/trackblock00.sav, trackblock01.sav
```

`save_0` appears to be one profile/container. The app should scan every
`save_*` directory and every numbered slot inside it, rather than assuming exactly
six slots or exactly one profile folder.

### 4.2 How the save is structured

Each `.sav` file (except tracker blocks) uses:

```
u32 version
u32 marker = 0xFFFFFFFF
u32 unknown = 1
[u32 id, u32 value] * N
u32 sentinel = 0xFFFFFFFF
```

The key ID is the CRC32 (IEEE) of the internal data name. Verified example:

```
CRC32("CurrentRupee") = 0x23149bf8 = 588553208
```

Values are interpreted according to the key's type in the game's save schema:
bool, s32, f32, string32/64/256, vector2f/3f/4f, and arrays of those types.
Strings and arrays repeat the same key ID for each 4-byte chunk.

Useful keys already identified in the supplied save:

| Key | ID | Type/use |
| --- | --- | --- |
| `PlayerSavePosMapName` | `0x0bee9e46` | current map name (e.g. `G-6`) |
| `CurrentRupee` | `0x23149bf8` | rupees |
| `MaxHeart` | `0x2906f327` | max hearts |
| `StaminaMax` | `0x3adff047` | max stamina (f32) |
| `PlayReport_PlayTime` | `0x73c29681` | play time, seconds |
| `KorokSeed` counter | `0x8a94e07a` | Korok seeds collected |
| `PlayerSavePos` | `0xa40ba103` | player position (vector3f) |
| `PlayerSavePosMapType` | `0xd913b769` | `MainField`, etc. |
| `MapTower_XX` | varies | tower activation flags |
| `Clear_DungeonXXX` | varies | shrine completion flags |
| `Location_*` | varies | discovered locations |
| `IsGet_MemoryPhoto_XXX` | varies | recovered memories |
| `Defeated_Enemy_*_Num` | varies | enemy defeat counters |
| `Npc_HiddenKorok*` | varies | individual Korok flags |
| `PorchItem*` | `0x5f283289` etc. | inventory |

The game's flag-name table (MIT-licensed community data) maps CRC32 IDs to
internal names, which lets us join save flags to map objects.

### 4.3 Parser design

Create a standalone `wayfinder-save` crate:

- `zerocopy` for zero-copy views of the header and 8-byte records.
- `binrw` for typed decoding of values, strings, arrays, and version-specific
  structures.
- A `SaveFile` abstraction exposing:
  - header/version validation
  - ordered chunk lookup by CRC32
  - typed accessors for scalars, strings, vectors, and arrays
  - a raw chunk map for diagnostics and unknown keys
- A schema/key layer with known key IDs and types. Unknown keys remain available.
- Flag mapping data for map objects and completion.
- No filesystem or database assumptions in the parser crate, so it is independently
  testable and reusable by the agent.

Parsing must handle:

- Partial writes while Savage Wind is saving (read, validate, retry with debounce).
- Rejecting unsupported/encrypted/big-endian files with a clear error.
- Version differences: 1.9.0 is primary; older known Switch versions may be
  accepted where the header and length are known.
- Multi-slot selection and auto-selection of the most recently modified slot.
- `caption.sav`/`caption.jpg` for slot summaries and thumbnails.
- `option.sav` and `tracker/trackblock*.sav` as later enhancements.

### 4.4 Saving and snapshots

- Watch the save folder with `notify`; debounce and hash changed files.
- Parse in `spawn_blocking` to keep the async runtime responsive.
- Store a snapshot containing:
  - captured time, game/save version, slot label
  - core stats and player position
  - completion flags for mapped objects
  - optional compressed raw save for re-parsing after schema updates
- Snapshots enable progress-over-time charts and a "today's record" style timeline.
- Raw saves are private by default and can be disabled/retained with a configured
  limit.

## 5. Savage Wind control server

### 5.1 What it provides

`SAVAGEWIND_CONTROL=<port>` opens a **TCP text/JSON** server on `127.0.0.1` only.
It is not HTTP; the server closes connections whose first line is an HTTP request.

Protocol:

- One request per line, one JSON reply per line, UTF-8, `\n` terminated.
- Multiple requests may be sent over one connection.
- Example reply: `{"ok":true,"data":{...}}`.

Read-only/player-safe commands relevant to Wayfinder Slate:

| Command | Data |
| --- | --- |
| `pos` | Link `x`, `y`, `z`, `stamina`, `place`, `player_info` |
| `state` | frame count, uptime, FPS, in-game hour |
| `camera` | camera position, direction, yaw, pitch |
| `version` | game version, OS, architecture |
| `get [key]` | current settings |
| `help` | command list |

`SAVAGEWIND_CONTROL` also unlocks developer commands. Wayfinder Slate and its agent
will only use the read-only player-safe commands above. The control port must stay
bound to loopback.

### 5.2 Coordinate axes

Savage Wind's position is reported as:

- `x` — left/right
- `y` — height/elevation
- `z` — forward/back (up/down on the map)

The Leaflet map uses `x` and `z` for horizontal placement; `y` is shown as elevation
and can be used for a live altitude readout. The exact axis order/sign relative to
the map tiles must be calibrated against known landmarks and Link's saved position.
Keep the transform in one module with tests and a configurable swap/sign if needed.

### 5.3 Local/LAN live mode

When the server runs on the gaming PC:

- The backend connects to `127.0.0.1:<port>` over TCP.
- It polls `pos`/`state`/`camera` at a configurable interval (about 1 Hz).
- It stores the latest telemetry in memory and broadcasts it to authenticated
  browser clients via SSE or WebSocket.
- The frontend shows a live Link marker, stamina, current place, in-game time, FPS,
  and a live/offline indicator.
- If Savage Wind is not running, the app degrades to the last save.

### 5.4 Remote hub live mode: push, not poll

The hub cannot reach a player's loopback control server. Use an outbound **push**
architecture instead:

```
Savage Wind (127.0.0.1:7777)
        │  TCP JSON (read-only commands)
        ▼
wayfinder-agent (player's PC)
        │  HTTPS POST / WebSocket (outbound only)
        ▼
Wayfinder Slate hub (public server)
        │  SSE / WebSocket
        ▼
Browser viewers
```

The agent:

- Runs on Windows and Linux alongside Savage Wind.
- Polls `pos`, `state`, and `camera` over the local TCP socket.
- Pushes batched telemetry to the hub at a configurable rate.
- Optionally watches the save folder and uploads snapshots to the hub.
- Authenticates with a per-profile agent token.
- Never listens on a port and never exposes the control server to the network.
- Handles Savage Wind not running, reconnects with backoff, and buffers briefly.
- Can be run manually from a terminal, as a service, or from a future tray app.

The hub:

- Accepts telemetry only from a valid agent token.
- Stores the latest telemetry per profile in memory and optionally a downsampled
  trail for a Hero's Path-like view.
- Broadcasts live updates to authorized viewers.
- Persists save snapshots, not every telemetry sample.
- Rate-limits and validates all agent input.

This is the preferred way to support live data on a remote hub. It also avoids
requiring inbound ports, port forwarding, or exposing the developer-capable control
socket.

## 6. Map assets

### 6.1 Source

Use the BotW map tiles and marker data from
<https://github.com/zeldadungeon/maps> (no code reuse, no fork).

Observed BotW assets:

- Tile zoom levels 0–5
- 750×750 px tiles
- 24,000×24,000 map at max zoom
- 32×32 tile grid at zoom 5
- Roughly 49 MB of BotW tiles and markers

The repository has no explicit license. Treat the tiles/markers as third-party
assets, credit Zelda Dungeon and Nintendo, and do not claim ownership. Prefer a
fetch script so the assets are not committed to git by default; the release binary
can still embed them.

### 6.2 Fetch pipeline

`scripts/fetch-map-assets.sh` (or a Rust `xtask`):

- Pins a specific zeldadungeon/maps commit.
- Downloads only `public/botw/tiles/**`, `public/botw/markers/**`, and required
  icons.
- Verifies SHA-256 checksums from a manifest.
- Writes to `frontend/static/map/botw/` (gitignored).
- Fails clearly if the upstream layout changes.
- Can be run in CI before the Vite build.

Attribution goes in `THIRD_PARTY_NOTICES.md` and the About page.

### 6.3 Leaflet setup

Use Leaflet with a custom simple CRS matching the tile scheme:

- Map size: 24,000
- Tile size: 750
- Scale: `750 / 24000 = 0.03125`
- Offset: `750 / 2 = 375`
- Transformation: `(scale, offset, -scale, offset)`
- Bounds: `[0, 24000]` on both axes
- Initial center: approximately `[-3750, -1900]` in map coordinates

Use `leaflet.markercluster` for the 900 Korok seeds and dense layers. Provide layer
toggles, completion filters, and a “nearest unfinished” action.

### 6.4 Completion data pipeline

Build a static `map_objects` dataset that joins:

- map marker positions and names (Zelda Dungeon markers)
- save flag IDs and types (MIT-licensed community flag data)
- game object IDs / CDungeon numbers where needed

Completion rules:

- **Koroks:** match marker coordinates to `Npc_HiddenKorok*` flag IDs; completed if
  the flag is set.
- **Shrines:** `Clear_DungeonXXX` flags; map CDungeon numbers to shrine markers
  using object-map/community data.
- **Towers:** `MapTower_XX` flags; map tower numbers to tower markers.
- **Memories:** `IsGet_MemoryPhoto_XXX` flags; map `MemoryXX` markers.
- **Divine Beasts:** relevant dungeon/quest flags; coordinates supplied separately.
- **Quests and treasure:** optional later phases.

The join must be generated/tested offline and committed as a small static JSON
artifact, not computed from game files at runtime.

## 7. Architecture

### 7.1 Stack

- **Frontend:** Svelte 5 + TypeScript + Vite + Leaflet
- **Backend:** Rust, axum, tokio, sqlx/SQLite, axum-login, Argon2id, rust-embed
- **Save parser:** Rust, binrw + zerocopy
- **Control client:** Rust async TCP JSON client
- **Agent:** small cross-platform Rust binary
- **Database:** SQLite, WAL mode, embedded sqlx migrations
- **Assets:** Vite build embedded into the server binary; map assets fetched before
  build

### 7.2 Repository layout

```
wayfinder-slate/
  Cargo.toml                 # workspace
  crates/
    wayfinder-save/          # save parser, schema, flag mapping
    wayfinder-control/       # Savage Wind TCP JSON client
    wayfinder-server/        # axum app, auth, DB, API, embedded UI
    wayfinder-agent/         # local push agent
  frontend/                  # Svelte 5 + Vite + Leaflet
  migrations/                # sqlx migrations
  scripts/
    fetch-map-assets.sh
  fixtures/
    saves/                   # synthetic / redacted test fixtures
  docs/
    PLAN.md
  THIRD_PARTY_NOTICES.md
  .gitignore
  README.md
```

### 7.3 Deployment modes

| Mode | Bind | Save access | Live control | Registration |
| --- | --- | --- | --- | --- |
| Local | `127.0.0.1` | Local folder watcher | Direct poll | Optional/local |
| LAN | `0.0.0.0` | Local folder watcher | Direct poll | Required |
| Hub | Behind reverse proxy | Upload or agent sync | Agent push | Open, rate-limited |

A single config controls mode, bind address, data directory, save paths, agent
settings, and feature flags.

### 7.4 Database schema (initial)

- `users` — id, username, password_hash (Argon2id), display_name, role, created_at
- `sessions` — tower-sessions SQLite store
- `profiles` — id, user_id, name, game_mode, is_public, share_global, created_at
- `save_slots` — id, profile_id, slot_index, label, source, last_modified,
  raw_hash, caption_blob, raw_blob (optional compressed), created_at
- `snapshots` — id, slot_id, captured_at, game_version, playtime_s, rupees,
  hearts, stamina, korok_count, position_x/y/z, map_name, map_type,
  stats_json, completion_json
- `agent_tokens` — id, profile_id, token_hash, label, created_at, last_seen_at,
  revoked_at
- `live_sessions` — profile_id, telemetry_json, updated_at
- `global_stats` — optional materialized aggregates by metric/period

Use UUIDv7/ULID IDs, foreign keys, indexes on profile/slot/timestamp, and cascade
deletes for user data removal. Store completion as a compact JSON object of arrays
per object type rather than thousands of rows per snapshot.

### 7.5 API outline

Auth:

- `POST /api/auth/register`
- `POST /api/auth/login`
- `POST /api/auth/logout`
- `GET /api/me`

Profiles and saves:

- `GET/POST /api/profiles`
- `PATCH/DELETE /api/profiles/:id`
- `POST /api/profiles/:id/saves/upload` (multipart)
- `POST /api/profiles/:id/saves/import-path` (local mode only)
- `GET /api/profiles/:id/slots`
- `GET /api/slots/:id`
- `GET /api/slots/:id/snapshot/latest`
- `GET /api/slots/:id/map`
- `GET /api/profiles/:id/stats`
- `GET /api/profiles/:id/history`

Agent and live:

- `POST /api/profiles/:id/agent-token`
- `DELETE /api/profiles/:id/agent-token/:token_id`
- `POST /api/agent/telemetry` (bearer token)
- `POST /api/agent/snapshot` (bearer token, multipart)
- `GET /api/profiles/:id/live` (SSE)
- `GET /api/config` (mode/feature flags)

Global and utility:

- `GET /api/global/stats`
- `GET /api/health`

Serve the SPA from `rust-embed` with an index fallback. In development, Vite proxies
`/api` to the Rust server.

### 7.6 Authentication and security

- Argon2id password hashing in `spawn_blocking`.
- Axum-login backend backed by sqlx; tower-sessions SQLite session store.
- Secure, HttpOnly, SameSite=Lax cookies in hub mode; configurable for local HTTP.
- Origin/CSRF checks for state-changing requests.
- Login and registration rate limiting; generic error messages.
- Agent tokens: high-entropy random tokens, stored hashed, shown once, revocable.
- Upload validation: size limits, header/version checks, file count limits, safe
  filenames.
- Public hub behind TLS reverse proxy; document Caddy/nginx examples.
- Private saves by default; global stats opt-in; user deletion removes snapshots
  and tokens.
- No inbound connection to the player's machine; agent is outbound-only.

### 7.7 Frontend pages

- **Login / Register**
- **Dashboard** — active slot summary, completion rings, recent snapshots
- **Map** — Leaflet Hyrule map, layer toggles, completion filters, live marker
- **Play Data** — stats, medals/titles, progress charts, slot history
- **Global** — opt-in aggregate stats and leaderboards
- **Profile / Saves** — slot list, upload/import, agent token, privacy settings
- **Settings** — display preferences, local mode save path
- **About** — attribution, version, data sources

Mobile-first layout, dark Sheikah Slate-inspired theme, responsive map controls.
Use Svelte stores plus a typed fetch client. Add a PWA manifest later if useful.

## 8. Milestones

### M0 — Plan and repository

- [x] Research Zelda Notes BotW features
- [x] Verify 1.9.0 save format with supplied save
- [x] Research Savage Wind control server and map assets
- [x] Write this plan
- [x] Initialise repo, `main` + `develop`

### M1 — Scaffold

- [x] Cargo workspace, Svelte/Vite frontend, dev scripts
- [x] Vite proxy, `rust-embed` release path
- [x] Map asset fetch script + attribution
- [x] CI checks (fmt, clippy, tests, frontend build)

### M2 — Save parser

- [x] `wayfinder-save` crate with binrw/zerocopy
- [x] Header/record/sentinel validation
- [x] Typed scalar/string/array accessors
- [x] Key ID metadata for core stats and inventory
- [x] Synthetic fixtures + integration test with 1.9.0 save
- [ ] Flag-to-map-object mapping
- [ ] Slot discovery and caption parsing

### M3 — Database and auth

- [ ] sqlx migrations and connection setup
- [ ] Users, profiles, sessions
- [ ] Register/login/logout, Argon2id
- [ ] Local/LAN/hub configuration

### M4 — Save ingestion

- [ ] Folder watcher with debounce/hash/retry
- [ ] Snapshot storage and retention
- [ ] Multipart upload and path import
- [ ] Slot selection API

### M5 — Dashboard and map

- [ ] Dashboard stats and completion
- [ ] Leaflet custom CRS and tiles
- [ ] Korok/shrine/tower/memory/Divine Beast layers
- [ ] Marker clustering, filters, progress
- [ ] Coordinate calibration tests

### M6 — Play Data and Global

- [ ] Medals/titles and progress charts
- [ ] Global opt-in aggregates
- [ ] Privacy controls

### M7 — Live mode and agent

- [ ] `wayfinder-control` TCP JSON client
- [ ] Local live polling and SSE
- [ ] Frontend live marker/stamina/place/time/FPS
- [ ] `wayfinder-agent` push binary
- [ ] Agent tokens, telemetry endpoint, hub broadcast
- [ ] Optional save snapshot sync via agent

### M8 — Hardening and release

- [ ] Rate limits, upload validation, origin checks
- [ ] 100% save test fixture
- [ ] Fuzz/malformed parser tests
- [ ] Docker/reverse-proxy docs
- [ ] Single-binary release build
- [ ] Attribution and licence review

## 9. Testing

- Parser unit tests: header, records, sentinel, strings, arrays, vectors, malformed
  input, unknown keys.
- Integration tests against the supplied 1.9.0 save (kept out of git).
- Optional 100% Switch save fixture for total-count validation; source legally and
  document attribution.
- Property/fuzz tests for panics and out-of-bounds reads.
- API tests with an in-memory/temporary SQLite database.
- Frontend component tests for map filters and dashboard.
- Playwright end-to-end smoke test: register, upload, view map, view stats.
- Coordinate calibration tests using known landmark coordinates.
- Security checks: authz on profiles/slots, token revocation, upload limits.

## 10. Risks and open questions

- **1.9.0 schema coverage** — the community documentation stops at 1.6.0. The
  supplied save verifies the header and many keys, but undocumented keys may need
  discovery. Mitigation: raw chunk map, diagnostics, fixtures.
- **Flag-to-marker mapping** — shrines/towers need CDungeon/tower number mapping.
  Mitigation: derive from object-map/community data and test against known saves.
- **Map tile licence** — the upstream repo has no explicit licence. Use with
  attribution, keep assets separate, and be prepared to swap the tile source.
- **Coordinate orientation** — x/z order and sign must be calibrated against the
  tiles. Mitigation: one transform module, known-landmark tests, visual check.
- **Public hub abuse** — open registration needs rate limits, validation, and
  monitoring. Mitigation: conservative defaults and admin tools.
- **Telemetry privacy** — live position is sensitive. Private by default, opt-in,
  authenticated viewers only, no public map by default.
- **Windows save discovery** — Savage Wind's Windows save folder sits next to the
  executable. Manual path configuration is required; auto-detect common locations
  where possible.
- **Master Mode/multiple profiles** — support arbitrary numbered slots and detect
  Master Mode where flags allow; do not assume six slots forever.
- **SQLite at hub scale** — WAL and indexes are sufficient initially; revisit if
  the hub grows beyond a single-node deployment.

## 11. Locked decisions

- Frontend: **Svelte 5 + TypeScript + Vite + Leaflet**.
- Backend: **axum + tokio + sqlx/SQLite + axum-login + Argon2id + rust-embed**.
- Save parser: **binrw + zerocopy**, Switch little-endian, 1.9.0 primary.
- v1 live mode: **local/LAN direct polling**; remote hub live via outbound
  **wayfinder-agent**.
- Map assets: **fetch script**, pinned upstream commit, checksums, attribution; no
  upstream code reuse.
- Public hub: **open registration**, **private saves by default**, **opt-in global
  stats**.
- Position axes: **x = left/right, y = height, z = forward/back**; map uses x/z.
- Branching: `main` stable, `develop` integration, feature branches from `develop`.

## 12. Repository setup

- `git init -b main` in `wayfinder-slate/`
- Local `user.name` = `Thomas Hodgkinson`
- Local `user.email` = `thomashodgkinson1987@gmail.com`
- Initial commit on `main`
- `git switch -c develop`
- All application work on `develop` or feature branches merged back into `develop`
