# Wayfinder Slate

A self-hosted companion web app for **Savage Wind**, the PC recompilation of
*The Legend of Zelda: Breath of the Wild*. It turns a BotW Switch 1.9.0 save into
a dashboard and interactive Hyrule map, with optional live position data from
Savage Wind's control server.

Wayfinder Slate is an unofficial fan project. It is not affiliated with Nintendo,
Zelda Dungeon, or the Savage Wind project.

## Status

Early scaffolding. The save parser, API, and UI are under active development.
See [`docs/PLAN.md`](docs/PLAN.md) for the full design and milestones.

## Requirements

- Rust 1.85+ (edition 2024)
- Node.js 20+ and npm
- Git (to clone the repository)

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
| `WAYFINDER_BIND` | `127.0.0.1:8080` | Listen address |
| `WAYFINDER_SAVE_DIR` | auto-detected | Savage Wind save folder |
| `WAYFINDER_CONTROL_ADDR` | `127.0.0.1:7777` | Savage Wind control socket |
| `WAYFINDER_DATA_DIR` | `./data` | SQLite database and uploads |
| `WAYFINDER_MAP_DIR` | bundled/none | Optional external map asset directory |

On Linux, saves default to `~/.local/share/savagewind/save`. On Windows, set
`WAYFINDER_SAVE_DIR` to the `save` folder next to `savagewind.exe`.

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
scripts/                   Map asset fetch and verification
docs/PLAN.md               Architecture and milestones
```
