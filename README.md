# Wayfinder Slate

A self-hosted companion web app for **Savage Wind**, the PC recompilation of
*The Legend of Zelda: Breath of the Wild*. It turns a BotW Switch 1.9.0 save into
a dashboard and interactive Hyrule map — Divine Beasts, shrines, towers,
memories, Koroks, and more — with optional live position data from Savage Wind's
control server.

Wayfinder Slate is an unofficial fan project. It is not affiliated with Nintendo,
Zelda Dungeon, or the Savage Wind project.

> [!NOTE]
> This project is developed primarily using AI. Most of the code and
> documentation are produced with large language models — mainly
> **GLM 5.3 Flash**, **DeepSeek V4.1 Flash**, and **MiniMax M3.1 Flash** —
> though this is not an exhaustive list.

## Status

Early scaffolding. The save parser, API, and UI are under active development.
See [`docs/PLAN.md`](docs/PLAN.md) for the full design and milestones.

## Requirements

- Rust 1.85+ (edition 2024)
- Node.js 22.12+ and npm (Node 26 "Lithium" LTS recommended)
- Git (to clone the repository)

Node.js 25 and newer link against `libatomic` on Linux, so older distributions
may need the `libatomic1` package installed. See [`.node-version`](.node-version)
for the version used by CI.

## Development

```sh
# 1. Fetch the BotW map tiles and marker data (~49 MB, not committed)
./scripts/fetch-map-assets.sh          # Linux / macOS
# scripts\fetch-map-assets.cmd         # Windows
# node scripts/fetch-map-assets.mjs    # any platform

# 2. Install frontend dependencies and build the UI
npm --prefix frontend install
npm --prefix frontend run build

# 3. Run the server
cargo run -p wayfinder-server
```

Open <http://127.0.0.1:8080>.

For frontend hot reload, run the Vite dev server in a second terminal:

```sh
npm --prefix frontend run dev
```

Vite proxies `/api` to the Rust server on `127.0.0.1:8080`.

## Configuration

| Variable | Default | Meaning |
| --- | --- | --- |
| `WAYFINDER_MODE` | `local` | `local`, `lan`, or `hub` |
| `WAYFINDER_BIND` | mode-aware | Listen address (see below) |
| `WAYFINDER_SAVE_DIR` | auto-detected | Savage Wind save folder |
| `WAYFINDER_CONTROL_ADDR` | `127.0.0.1:7777` | Savage Wind control socket |
| `WAYFINDER_DATA_DIR` | `./data` | SQLite database and uploads |
| `WAYFINDER_COOKIE_SECURE` | `false` | Mark the session cookie `Secure` (set for HTTPS) |
| `WAYFINDER_SNAPSHOT_LIMIT` | `500` | Max stored snapshots per save slot (`0` keeps all) |
| `WAYFINDER_KEEP_RAW_SAVES` | `false` | Store gzip-compressed copies of `game_data.sav` |
| `WAYFINDER_WATCH` | `true` | Watch the save folder for changes (never in hub mode) |
| `WAYFINDER_MAX_UPLOAD_BYTES` | `8388608` | Max multipart upload size (clamped to 512 KiB minimum) |
| `WAYFINDER_MAP_DIR` | bundled/none | Optional external map asset directory |

The default bind address follows the deployment mode: `local` binds
`127.0.0.1:8080` (loopback only), while `lan` and `hub` bind `0.0.0.0:8080` so
other devices can connect.

On Linux, saves default to `~/.local/share/savagewind/save`. On Windows, set
`WAYFINDER_SAVE_DIR` to the `save` folder next to `savagewind.exe`.

### Authentication

Accounts are stored in the SQLite database under `WAYFINDER_DATA_DIR`
(`wayfinder.sqlite3`), and passwords are hashed with Argon2id. The schema and
migrations are embedded in the server binary and applied automatically on
startup.

- **Local** mode needs no login: the whole UI works on loopback.
- **LAN** and **hub** modes require a session, and save data is hidden until you
  sign in. The first account registered on a fresh database becomes the
  administrator; later accounts are ordinary users.
- Registration and login are handled by `POST /api/auth/register` and
  `POST /api/auth/login`; `POST /api/auth/logout` ends the session. The session
  cookie lives for seven days of inactivity.
- Set `WAYFINDER_COOKIE_SECURE=true` when serving over HTTPS (for example
  behind a TLS-terminating reverse proxy). Leaving it `false` on plain HTTP
  keeps login working on a LAN address without TLS.

### Live mode

Savage Wind exposes a read-only control socket when started with:

```sh
SAVAGEWIND_CONTROL=7777 ./savagewind
```

In local and LAN mode the server polls `pos`, `state`, and `camera` from that
socket. The socket is loopback-only; on a remote hub, live data will be pushed by
a local `wayfinder-agent`, never by inbound connections to the player's PC.

### Probe the control socket

```sh
cargo run -p wayfinder-agent -- probe
```

### Saved data and snapshots

Save data is ingested once and kept in SQLite; the API reads stored snapshots
and never re-parses `game_data.sav` on each request. The same read path serves
local, LAN, and hub deployments.

- On startup the server scans `WAYFINDER_SAVE_DIR` and ingests every numbered
  slot. In local and LAN mode a watcher then rescans after changes (750 ms quiet
  window, 2 s cap), deduping by SHA-256 so an unchanged save adds no history.
- Each distinct save state adds a snapshot. `WAYFINDER_SNAPSHOT_LIMIT` caps
  history per slot; `0` keeps everything.
- Set `WAYFINDER_KEEP_RAW_SAVES=true` to also store a gzip-compressed copy of
  each `game_data.sav`. Raw blobs are never exposed by the API and cost roughly
  1 MB per save.
- In hub mode, `/api/slots` reads the signed-in user's primary profile. Use the
  **Saves** page to upload `game_data.sav` (required) plus optional `caption.sav`
  and `caption.jpg` into any owned profile, and to delete stored profile slots
  (`DELETE /api/profiles/{id}/slots/{slot}` removes the slot and its snapshots;
  machine-local slots always mirror the watched folder and cannot be deleted
  through the API).
- `POST /api/saves/import-path` imports a folder's slots into the machine-local
  store. It is available only in local mode; LAN and hub return `403`.

## Map assets

Tiles and marker data come from the
[Zelda Dungeon maps repository](https://github.com/zeldadungeon/maps), pinned to
a specific commit and verified against `scripts/map-assets.sha256`. We use the
assets only, not their application code. See
[`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).

The fetcher is a dependency-free Node script, so it works on Windows, Linux, and
macOS without Git or Unix tools. Use `--verify-only` to check an existing
download, `--force` to re-download, or `--write-manifest` to rebuild the checksum
manifest after changing the pinned commit.

## Repository layout

```
crates/wayfinder-save/     BotW Switch save parser
crates/wayfinder-control/  Savage Wind control socket client
crates/wayfinder-server/   axum web server and embedded UI
crates/wayfinder-agent/    outbound live-telemetry agent
frontend/                  Svelte 5 + Vite + Leaflet UI
scripts/                   Map asset fetch, dataset build, and verification
docs/PLAN.md               Architecture and milestones
```

## Contributing

Contributions are welcome. Read [`CONTRIBUTING.md`](CONTRIBUTING.md) for the
branching model, development checks, save-data rules, and pull-request guidance.

## License

Wayfinder Slate's source code is released under the MIT License. See
[`LICENSE.md`](LICENSE.md).

The MIT License applies only to this project's source code. Map tiles, marker
data, and icons fetched from Zelda Dungeon remain third-party material under
their own terms; see [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).
