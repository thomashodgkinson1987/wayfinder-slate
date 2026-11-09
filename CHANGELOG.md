# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Wayfinder Slate is in initial development (`0.y.z`). Per the SemVer guidance for
major version zero, the minor version is bumped for each development release and
the patch version for fix-only releases. The public API should not be considered
stable until `1.0.0`.

## [Unreleased]

### Added

- Save ingestion: a startup scan and a debounced folder watcher ingest
  `game_data.sav` into new `save_slots` and `snapshots` tables, deduping by
  SHA-256 and retrying partial writes.
- Snapshot storage with configurable per-slot retention
  (`WAYFINDER_SNAPSHOT_LIMIT`, default 500) and optional gzip raw-save retention
  (`WAYFINDER_KEEP_RAW_SAVES`).
- API: `POST /api/profiles/{id}/saves/upload` (multipart `game_data`,
  `caption_sav`, `caption_image`, optional `slot_index`) and
  `GET /api/profiles/{id}/slots` plus `.../{slot}/caption`.
- API: `POST /api/saves/import-path` imports a save folder in local mode.
- UI: a Saves page for profile uploads and local folder imports.

### Changed

- Save APIs now read stored snapshots instead of parsing `game_data.sav` on
  every request.
- Hub mode now shows a signed-in user's primary-profile saves; previously it
  served no save data at all.

## [0.6.0] - 2026-11-09

### Added

- Map: Divine Beast layer and completion, derived from the four `Clear_Remains*`
  save flags and pinned to the upstream Main Quest markers.
- API: `GET /api/slots/{slot}/map` returns the per-object completion identifiers
  (flag names for named objects, CRC32 hashes for treasure chests) for a save
  slot.
- Map: `leaflet.markercluster` clustering for the dense layers, an
  All / Unfinished / Completed completion filter, per-layer progress bars with
  an overall completion percentage, and a "nearest unfinished" action.
- Frontend: vitest + jsdom test runner with coordinate calibration tests
  (independent upstream marker coordinates, world corners, and bounds
  round-trips) and unit tests for the completion/filter/nearest helpers, run in
  CI.

### Fixed

- Map: load `leaflet.markercluster` after seeding the global Leaflet instance,
  fixing a blank page caused by `ReferenceError: L is not defined` at startup.
- Build: re-embed `frontend/dist` when it changes, so `cargo build` cannot reuse
  a binary with stale frontend assets.

### Changed

- Dashboard: show Divine Beasts, locations, and enemies alongside the existing
  completion cards; Play Data now shows Divine Beasts too.

## [0.5.0] - 2026-11-09

### Added

- Server: SQLite persistence under `WAYFINDER_DATA_DIR` with embedded,
  auto-applied migrations, running in WAL mode.
- Authentication: account registration, login, and logout backed by Argon2id
  password hashing and server-side session storage. The first registered account
  becomes the administrator and later accounts are ordinary users; each account
  gets a default profile.
- Access policy: `local` mode stays open, while `lan` and `hub` modes require a
  session for save data. Per-user endpoints (`/api/me`, `/api/profiles`) require
  a signed-in user in every mode.
- Frontend: sign-in / create-account view for LAN and hub deployments, and a
  signed-in account card with a sign-out button in the sidebar.
- Configuration: mode-aware default bind address (`127.0.0.1` for local,
  `0.0.0.0` for LAN/hub) and `WAYFINDER_COOKIE_SECURE` for HTTPS deployments.
- API: `/api/config` now reports `auth_required` and `registration_open`.

## [0.4.0] - 2026-11-08

### Added

- Save parser: player stats — play time, rupees, hearts, stamina, and current
  location — parsed from save slots.
- Save parser: caption metadata (saved-at timestamp, location, and map area)
  for save slot summaries.
- Server: save slot discovery across `save_*` profile folders, exposing slot
  lists, per-slot summaries, and caption thumbnails.
- Frontend: slot picker and real save statistics on the dashboard.
- Map: treasure chest layer covering the 1,312 static chest placements, joined
  to save flags by CRC32 hash via `wayfinder-save`'s generated hash table and
  `scripts/treasure-flags.json`, with a new `scripts/build-treasure-objects.mjs`
  dataset builder.

### Changed

- Toolchain now targets Node.js 26 LTS and requires Node.js 22.12+ (CI,
  `.node-version`, documentation, and fetch scripts).

### Fixed

- Map marker coordinates are aligned with the tile grid, and the map now fills
  the viewport on mobile.
- Korok seed markers are labelled by seed number instead of area name.

## [0.3.0] - 2026-11-08

### Added

- `map_objects` dataset (`frontend/static/data/map_objects.json`) joining
  Zelda Dungeon markers to save flags for shrines, towers, Koroks, memories,
  locations, and enemies, with invariant checks in
  `scripts/validate-map-objects.mjs`.
- Completion API endpoints `/api/map-objects` and `/api/completion`.
- Map layers and the Play Data page now reflect completion state read from the
  save.

### Fixed

- M2 map integration repairs and completion edge cases in the save parser,
  server configuration, API responses, and frontend.

## [0.2.0] - 2026-11-07

### Added

- Cargo workspace with four crates: `wayfinder-save` (binrw/zerocopy parser for
  BotW Switch 1.9.0 saves), `wayfinder-control` (async TCP JSON client for the
  Savage Wind control socket), `wayfinder-server` (axum web server serving the
  embedded UI, with environment-based configuration), and `wayfinder-agent`
  (CLI with a `probe` subcommand).
- Svelte 5 + TypeScript + Vite frontend with a Leaflet map using a custom CRS
  for the Zelda Dungeon tile scheme, plus Dashboard, Play Data, and Setup
  pages.
- Map asset fetch pipeline with a checksum manifest
  (`scripts/map-assets.sha256`).
- CI workflow running `cargo fmt`/`clippy`/`test` and the frontend
  check/build.
- Project documentation: README, third-party notices, editor configuration,
  MIT License (`LICENSE.md`), and contributing guide (`CONTRIBUTING.md`).

### Changed

- Replaced the shell-only map asset fetcher with a dependency-free,
  cross-platform Node script (`scripts/fetch-map-assets.mjs`) with `.sh` and
  `.cmd` wrappers, so it runs on Windows, Linux, and macOS without Git or Unix
  tools.

## [0.1.0] - 2026-11-07

### Added

- Project plan (`docs/PLAN.md`) covering the product vision, BotW save-format
  findings, Savage Wind control server protocol, map asset pipeline,
  architecture, milestones, and repository setup.

[Unreleased]: https://github.com/thomashodgkinson1987/wayfinder-slate/compare/v0.6.0...HEAD
[0.6.0]: https://github.com/thomashodgkinson1987/wayfinder-slate/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/thomashodgkinson1987/wayfinder-slate/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/thomashodgkinson1987/wayfinder-slate/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/thomashodgkinson1987/wayfinder-slate/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/thomashodgkinson1987/wayfinder-slate/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/thomashodgkinson1987/wayfinder-slate/releases/tag/v0.1.0
