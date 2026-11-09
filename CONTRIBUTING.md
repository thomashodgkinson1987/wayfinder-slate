# Contributing to Wayfinder Slate

Thanks for taking an interest. Wayfinder Slate is an unofficial companion for
Savage Wind, built around read-only save data and Savage Wind's documented,
player-safe control commands.

Please read [`README.md`](README.md) and [`docs/PLAN.md`](docs/PLAN.md) before
starting significant work. The plan records the v1 scope, architecture, and
locked decisions.

## Scope and ground rules

- Keep v1 read-only. Do not add save writing, memory editing, item injection, or
  undocumented developer commands.
- Only use Savage Wind control commands marked player-safe. Never expose the
  loopback control socket to the network.
- Do not add voice-memory audio, amiibo effects, daily bonuses, item sharing, or
  Photo Studio features.
- Public-hub behaviour must remain private-by-default, with global statistics
  strictly opt-in.
- Do not copy application code from `zeldadungeon/maps` or other map projects.
  Wayfinder Slate uses the attributed map assets only.

## Branching

- `main` is the stable branch.
- `develop` is the integration branch and the default target for pull requests.
- Create feature branches from `develop`, for example:
  - `feat/save-slot-discovery`
  - `fix/map-layer-toggle`
  - `docs/hub-deployment`

Use clear, conventional commit messages such as `feat:`, `fix:`, `docs:`,
`test:`, `refactor:`, and `chore:`.

## Development setup

Requirements: Rust 1.85+, Node.js 22.12+ (Node 26 "Lithium" recommended), and npm.

The repository ships a [`.node-version`](.node-version) file, so version managers
such as fnm, nodenv, or asdf pick up the recommended Node.js release
automatically.

```sh
# Map assets (~50 MB, downloaded and checksum-verified; not committed)
./scripts/fetch-map-assets.sh          # Linux / macOS
# scripts\fetch-map-assets.cmd         # Windows
# node scripts/fetch-map-assets.mjs    # any platform

# Frontend
npm --prefix frontend install
npm --prefix frontend run build

# Server
cargo run -p wayfinder-server
```

For frontend hot reload, run `npm --prefix frontend run dev` and let Vite proxy
`/api` to the Rust server on port 8080.

## Checks before opening a pull request

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm --prefix frontend run check
npm --prefix frontend run test
npm --prefix frontend run build
node --check scripts/fetch-map-assets.mjs
```

If you are working on the save parser, also run it against a real save without
committing that save:

```sh
WAYFINDER_TEST_SAVE=/path/to/game_data.sav \
  cargo test -p wayfinder-save --test real_save
```

## Code guidelines

### Rust

- Use edition 2024 and keep the workspace crates focused:
  - `wayfinder-save` parses bytes and knows nothing about HTTP or SQLite.
  - `wayfinder-control` owns the Savage Wind TCP JSON protocol.
  - `wayfinder-server` owns configuration, persistence, auth, and HTTP policy.
  - `wayfinder-agent` is outbound-only and never listens for connections.
- Run expensive parsing in `spawn_blocking`; do not block the async runtime.
- Prefer typed errors at crate boundaries and `anyhow` only in binaries.
- Add tests for malformed input, version handling, and privacy-sensitive paths.

### Svelte and TypeScript

- Use Svelte 5 runes and strict TypeScript.
- Keep the UI mobile-first, keyboard-accessible, and usable without a mouse.
- Do not present placeholder data as real save data.
- Keep map coordinate conversion in `frontend/src/lib/coords.ts`. The convention
  is Leaflet `lat = -z`, `lng = x`; `y` is elevation only.

### API and privacy

- Authenticate and authorise every profile, save, and telemetry request.
- Keep saves private by default and never include user data in global aggregates
  without explicit opt-in.
- Avoid logging save contents, live positions, tokens, or password material.
- Validate uploads and cap size, file count, and telemetry rates.

## Save data and test fixtures

Never commit a real Breath of the Wild save, `caption.sav`, `caption.jpg`,
`option.sav`, tracker blocks, or screenshots containing personal data. The
supplied development save lives outside this repository and is referenced with
`WAYFINDER_TEST_SAVE`.

Synthetic fixtures are welcome. Keep them small, clearly documented, and free of
personal information. Use `fixtures/saves/private/` for local-only files; it is
gitignored.

## Map assets

The map fetcher pins a Zelda Dungeon commit and verifies every file against
`scripts/map-assets.sha256`. Downloaded assets are gitignored. Do not commit
them, and update [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md) if the source
or attribution changes.

## Pull requests

A good pull request is focused and includes:

- A short description of the problem and the chosen approach.
- Tests for new parser, API, auth, or map behaviour.
- Updated documentation where configuration or behaviour changes.
- No generated build output, downloaded map assets, real saves, or secrets.
- A note on privacy or security implications for live telemetry, uploads, or
  global statistics.

If a change touches authentication, uploads, agent tokens, or public data access,
explain the threat model in the pull request.

## Reporting bugs and security issues

For normal bugs, open an issue with the version, deployment mode, platform, steps
to reproduce, and relevant logs with secrets and personal data removed.

Please do not disclose exploitable security issues in a public issue. Use the
repository host's private security-advisory feature if available, or contact the
maintainer privately before sharing details.

## Licence

By contributing, you agree that your contribution is licensed under the MIT
Licence in [`LICENSE.md`](LICENSE.md). No separate contributor licence agreement
is required.
