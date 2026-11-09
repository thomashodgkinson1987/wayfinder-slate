# Wayfinder Slate — M3: Database and Auth — Implementation Plan

> Written: 2026-11-09 (dependency research done same day)
> Target repo: `/workspace/wayfinder-slate`
> Branch: `feat/m3-database-auth` created from `develop`
> Self-contained hand-off plan for a fresh build session. Plan decisions are
> settled — do not re-litigate them; implement.

## 0. Orientation (read first)

- `/workspace` is **not** the repo root. The repo lives entirely in
  `/workspace/wayfinder-slate`. Read `/workspace/INDEX.md` for orientation.
- `/workspace/maps/` (zeldadungeon/maps clone) and `/workspace/save/` (real BotW
  save sample, personal data) are **read-only reference**. Never modify, never
  commit, never redistribute the save.
- Read `README.md`, `CONTRIBUTING.md`, and `docs/PLAN.md` (esp. §7.4 database
  schema, §7.5 API outline, §7.6 auth, §8 milestones, §11 locked decisions)
  before writing code.
- Milestone to implement (`docs/PLAN.md` §8, M3):
  - [ ] sqlx migrations and connection setup
  - [ ] Users, profiles, sessions
  - [ ] Register/login/logout, Argon2id
  - [ ] Local/LAN/hub configuration
  - Tick these boxes in `docs/PLAN.md` when done.
- Workflow already established: Milestones M0–M2 and parts of M5 are complete
  (`0.4.0` on `develop`). The server currently serves save data straight from
  the filesystem with **no database and no auth** — M3 adds the persistence +
  auth foundation that M4 (save ingestion) builds on.

## 1. Environment facts

- Passwordless-sudo Arch Linux container. Toolchain present:
  `rustc 1.99.1`, `cargo 1.99.1`, `node 26.13.0`, `npm 12.2.2`.
- Frontend deps are already installed-able via `npm --prefix frontend install`;
  no new npm packages are needed for M3.
- Map assets for running the app locally:
  `./scripts/fetch-map-assets.sh` (~49 MB, gitignored).
- `target/debug` builds exist; first `cargo build` may take a while.
- Every user-facing check must pass (see §9 Verification).

## 2. Settled decisions (with rationale)

### 2.1 Dependency strategy — custom session store

**Decision:** upgrade to `sqlx 0.9`, keep `axum-login 0.18` (plan-locked), and
implement the tower-sessions `SessionStore` trait ourselves against our own
sqlx 0.9 SQLite pool, backed by a `sessions` table in our database. Do **not**
use `tower-sessions-sqlx-store`.

**Why (verified on crates.io + GitHub on 2026-11-09):**

| crate | latest | situation |
| --- | --- | --- |
| `sqlx` | 0.9.0 | latest, stable (MSRV 1.94; we have 1.99.1). Released 2026-05-06. |
| `axum` | 0.8.9 | workspace already on `"0.8"` → resolves latest |
| `axum-login` | 0.18.0 | pins `tower-sessions ^0.14` → blocks tower-sessions 0.15 |
| `tower-sessions` | 0.15.0 exists, **unusable** | use **0.14.0** (forced by axum-login 0.18) |
| `tower-sessions-sqlx-store` | 0.15.1 | requires `tower-sessions-core ^0.15` (blocked) **and** `sqlx ^0.8` → would drag a second full copy of sqlx + libsqlite3-sys into the binary |
| `argon2` / `password-hash` | 0.6.0 / 0.6.1 | fine; rand_core ^0.10 aligns with rand 0.10 |
| `uuid` / `time` / `rand` | 1.29.2 / 0.3.55 / 0.10.3 | fine |

**Upstream outlook** (for the user's follow-up, verified via GitHub):
- `maxcountryman/tower-sessions-stores` `main` already merged “Update sqlx to
  0.9.0” (2026-07-12; PRs #90/#91). Published crates.io lags (0.15.1 still
  points at sqlx ^0.8 / had the core-version mismatch reported in issue #83).
  Maintainer merged **“Add publish CI” on 2026-11-08** because published crates
  drifted from `main` → a re-publish wave is likely but **no announced date**.
- `maxcountryman/axum-login` `main` already upgraded to tower-sessions 0.15
  (2026-05-07, PR #315) and finalized a **breaking** “require builder API”
  (2026-01-10, #311). No crates.io release since 0.18.0. Open PR #327 bumps its
  sqlx to 0.9.
- Conclusion: everything needed upstream exists on `main`; only **releases**
  are missing, timing unknown. Do not pin git `main` deps.

**Upgrade path (M8 or when releases land):** bump axum-login (next release),
adapt to its new builder API if still needed, then optionally delete the custom
store and swap in the official `tower-sessions-sqlx-store` once it targets
sqlx ^0.9 + core ^0.15. Our store deliberately mirrors the official crate's
schema and MessagePack data format so swap costs stay low. Session rows may be
dropped harmlessly if formats drift (users just re-login).

### 2.2 Auth policy per mode (from PLAN §7.3/§11 — locked)

- **Local** (default): registration optional. Anonymous users can browse
  current behaviour (save slots, completion) on loopback; login/register are
  available but not required. This preserves today's UX on the gaming PC.
- **LAN / Hub**: auth **required** for save/profile data (JSON `401`, not a
  redirect). Registration open (rate limiting & origin checks deferred to M8).
- Always public without auth in all modes: `/api/health`, `/api/config`,
  `/api/map-objects`, static SPA assets, and the auth endpoints themselves.
- Auth-gated in **all** modes (even local): `/api/me`, `/api/profiles*` —
  these are per-user resources.
- First registered user gets `role = admin`; subsequent users `user`.
- Registration auto-creates one `profiles` row named `Default` per user.

### 2.3 Other settled choices

- `sqlx` used with **runtime queries only** (`query()`, `query_as()`), no
  `query!()` macros → no `DATABASE_URL`, no offline `.sqlx` artifacts, CI stays
  DB-free. Correctness covered by tests.
- Migrations live at the **repo root** `migrations/` (per PLAN §7.2 layout),
  embedded via `sqlx::migrate!("../../migrations")` (path relative to the
  `wayfinder-server` crate manifest dir).
- DB file: `$WAYFINDER_DATA_DIR/wayfinder.sqlite3` (default `./data`,
  gitignored — verify `.gitignore` covers it).
- Session TTL: 7 days via tower-sessions `Expiry::Age(7 days)` so records get a
  concrete expiry timestamp we can store/filter. Cookie `max-age` follows.
- Cookies: `HttpOnly` (default), `SameSite=Lax` set explicitly, `Secure` false
  by default (plain HTTP on loopback/LAN) with `WAYFINDER_COOKIE_SECURE=true`
  override for TLS hub deployments. Document that hub deployments belong behind
  a TLS reverse proxy (full hardening M8).
- Passwords hashed with **Argon2id**, explicit params m=19 MiB (19456 KiB),
  t=2, p=1 (OWASP baseline — don't trust the crate default blindly, set them).
  Hash/verify run in `tokio::task::spawn_blocking` (CONTRIBUTING rule).
- `session_auth_hash` returns the PHC password-hash string bytes (same trick
  as upstream examples: changing the password invalidates all sessions).
- Login failures return a **generic** `401 {"error":"invalid credentials"}` —
  never disclose whether the username exists.
- Profile management API this milestone: `GET/POST /api/profiles` only.
  `PATCH/DELETE` profile endpoints are deferred to M4 (not just left stubs).
- Unique username: compare case-insensitively; store as given, enforce with a
  caller-side `LOWER(username)` check + `UNIQUE` index on `LOWER(username)`.
- Validation: username `^[A-Za-z0-9_-]{3,24}$`; password 8–128 chars;
  display_name optional ≤48 chars (defaults to username). Errors →
  `400 {"error":"…"}`; duplicate username → `409`.

## 3. Dependencies to add

All in `[workspace.dependencies]`, then referenced as `<name>.workspace = true`
in `crates/wayfinder-server/Cargo.toml`:

```toml
# workspace Cargo.toml additions
axum-login = "0.18"
tower-sessions = "0.14"          # pinned by axum-login 0.18 (see §2.1)
sqlx = { version = "0.9", features = ["runtime-tokio", "sqlite", "migrate", "time"] }
argon2 = { version = "0.6", features = ["password-hash"] }   # verify exact feature names at impl; may need "std"
rmp-serde = "1.1"                # session blob MessagePack (compatible with official store)
uuid = { version = "1.29", features = ["v7"] }
time = { version = "0.3.55", features = ["formatting", "parsing"] }  # only what sqlx/store needs
rand = "0.10"                    # salt generation via password-hash
# dev-dependencies (server crate): tempfile = latest 0.3.x for sqlite test files
```

Notes:
- sqlx 0.9 removed combined `runtime-<rt>-<tls>` features; `runtime-tokio`
  alone is correct, and we need no TLS (local SQLite).
- `tower-sessions` re-export is available as `axum_login::tower_sessions`;
  depending on it directly is also fine for `SessionStore` impl.
- Do not commit lockfile churn beyond the new deps: `cargo update -p <crate>`
  style upgrades of existing pins are fine if needed for resolution, but keep
  the commit focused. Existing direct deps (anyhow, axum, tokio, tower-http,
  thiserror, serde...) are already caret-ranged → cargo picks latest.

## 4. Work breakdown

### 4.1 New module layout in `crates/wayfinder-server/src/`

```
src/
  main.rs        # create data dir, open pool, run migrations, wire router fields
  db.rs          # SqliteConnectOptions (WAL, foreign_keys ON, busy_timeout ~5s),
                 # SqlitePoolOptions::after_connect to PRAGMA foreign_keys,
                 # migrate!("../../migrations").run(&pool), purge expired sessions at startup
  auth/
    mod.rs       # module glue, Backend type alias, AuthSession alias
    users.rs     # User model + AuthUser impl, Users backend store (sqlx queries),
                 # Credentials, AuthnBackend impl, argon2 hash/verify helpers
                 # (spawn_blocking), input validation helpers
    session_store.rs  # SqlxSessionStore + impl tower_sessions::SessionStore (+ tests)
  config.rs      # existing file; mode-aware bind default, cookie_secure flag,
                 # session TTL constant
  http.rs        # existing; new routes + auth/policy layers + tests
migrations/
  00000000000001_m3_auth.sql   # (sqlx naming: <version>_<description>.sql)
```

### 4.2 Migration 0001 (schema per PLAN §7.4)

```sql
CREATE TABLE users (
    id            TEXT PRIMARY KEY,                 -- UUIDv7 as UUID string
    username      TEXT NOT NULL,
    password_hash TEXT NOT NULL,                    -- PHC string (Argon2id)
    display_name  TEXT NOT NULL,
    role          TEXT NOT NULL DEFAULT 'user' CHECK (role IN ('admin', 'user')),
    created_at    INTEGER NOT NULL                  -- unix seconds
);
CREATE UNIQUE INDEX users_username_unique ON users (LOWER(username));
-- Case-insensitive uniqueness: pick exactly ONE mechanism. Either mark the
-- column `username TEXT NOT NULL UNIQUE COLLATE NOCASE` directly, or keep the
-- expression index above — do not use both.

CREATE TABLE profiles (
    id           TEXT PRIMARY KEY,
    user_id      TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    game_mode    TEXT NOT NULL DEFAULT 'normal' CHECK (game_mode IN ('normal', 'master')),
    is_public    INTEGER NOT NULL DEFAULT 0,        -- SQLite booleans as INTEGER
    share_global INTEGER NOT NULL DEFAULT 0,
    created_at   INTEGER NOT NULL
);
CREATE INDEX idx_profiles_user ON profiles (user_id);

CREATE TABLE sessions (
    id     TEXT PRIMARY KEY,
    data   BLOB NOT NULL,   -- rmp-serde MessagePack of SessionData (official-store compatible)
    expiry INTEGER NOT NULL -- unix seconds
);
CREATE INDEX idx_sessions_expiry ON sessions (expiry);
```

(Clean up the commented notes above before committing.)

### 4.3 Config changes (`config.rs`)

- New fields: `cookie_secure: bool` (env `WAYFINDER_COOKIE_SECURE`, default
  false), `session_ttl` constant 7 days (module const, not env — keep env
  surface small). `data_dir` already exists.
- Bind default becomes mode-aware: local → `127.0.0.1:8080`, lan/hub →
  `0.0.0.0:8080`, explicit `WAYFINDER_BIND` still overrides.
- Update the existing `Config` struct usages/tests; test constructors must
  compile (they currently list fields explicitly).

### 4.4 Users backend (`auth/users.rs`)

- `User { id, username, password_hash, display_name, role, created_at }`.
- `impl AuthUser for User`: `type Id = String` (or a newtype over the UUID);
  `session_auth_hash()` = `self.password_hash.as_bytes()`.
- `Backend`/store struct `Users { pool: SqlitePool }` implementing
  `AuthnBackend`:
  - `type Credentials = Credentials { username, password }`
  - `authenticate`: `SELECT ... WHERE LOWER(username) = LOWER(?)`, then verify
    Argon2id in `spawn_blocking`; unknown user → run a cheap dummy verify once
    to keep timing roughly uniform (worth adding, small cost) — optional.
  - `get_user`: by UUID string id.
  - `type Error = sqlx::Error` (non-Infallible is supported; map handler-side).
- Mutation helpers in the same module (called by handlers, not part of trait):
  `insert_user`, `username_exists`, role assignment via `COUNT(*) == 0 → admin`
  inside the insert transaction, `create_default_profile`.
- axum-login 0.18 confirmed API (docs.rs): traits `AuthUser`/`AuthnBackend`/
  `AuthzBackend`; extractor `AuthSession<Backend>` with `.authenticate()`,
  `.login(&user)`, `.logout()`, `.user`; layer via
  `AuthManagerLayerBuilder::new(backend, SessionManagerLayer::new(store)).build()`.
  The macro `login_required!` **redirects** to a login URL — wrong for our JSON
  API; use a custom `axum::middleware::from_fn_with_state` guard instead
  (§4.6). (The `AuthnLayer`/`AuthzLayer` require-builder API is unreleased
  upstream — ignore it.)

### 4.5 Session store (`auth/session_store.rs`)

- `pub struct SqlxSessionStore(SqlitePool)`.
- Implement `tower_sessions::SessionStore` (0.14). Confirm exact signatures
  from docs.rs before writing (method set ≈ `create`, `save`/`renew`,
  `load`, `delete`, + optional `ExpiryDeletion`); mirror the official
  `tower-sessions-sqlx-store` behaviour:
  - blob via `rmp_serde::to_vec(record)` / `from_slice` (`SessionRecord`
    and/or `SessionData` — match official store's field granularity)
  - `load` filters `expiry > now`
  - `create` inserts; `save` upserts (`INSERT OR REPLACE` for SQLite)
  - expiry stored as unix seconds; treat `Expiry::Age` concrete expiry
- Startup purge: `DELETE FROM sessions WHERE expiry < unixtime-now` in `db.rs`.
- Unit tests mirroring the official crate's suite: create→load roundtrip,
  save updates, delete, expired session not returned, purge removes expired.

### 4.6 Router + policy (`http.rs`, `main.rs`)

- Build in `main.rs`: data dir → pool → migrations → purge → `Arc<Config>` +
  `Arc<SqlitePool>` into router (extend `AppState` — config struct plus pool).
  Update `main.rs`'s `Config` destructure accordingly.
- Layers (order matters, session layer before authn layer):
  `CompressionLayer`/`TraceLayer` + `SessionManagerLayer`
  `.with_secure(config.cookie_secure).with_same_site_policy(SameSite::Lax)`
  (HttpOnly is the cookie default) + `AuthManagerLayerBuilder`.
- Mode policy guard: one `from_fn_with_state` middleware `require_auth_if_lan_or_hub`
  (applied with `route_layer` to the save-data routes: `/api/slots*`,
  `/api/completion`): if `mode` is Lan/Hub and no authenticated user →
  `401 {"error":"authentication required"}`. Also gate `/api/me`,
  `/api/profiles*` in all modes (same guard, a second variant).
  Return JSON 401s everywhere; never redirect (SPA talks JSON).
- New routes:
  - `POST /api/auth/register` → validate, check unique username (409), hash
    password (spawn_blocking), insert user (+ role via first-user rule),
    create default profile, create session
    (`auth_session.login(&user).await`), respond `201 {"user": {...}}`.
  - `POST /api/auth/login` → authenticate; ok → `200 {"user": ...}`; invalid →
    generic `401 {"error":"invalid credentials"}`; backend error → 500.
  - `POST /api/auth/logout` → `auth_session.logout()`, clear/expire session
    record (tower-sessions flush semantics) → `204` (even when not logged in).
  - `GET /api/me` → `200 {user}` or `401`.
  - `GET /api/profiles` → caller's profiles (auth required) → `200`; empty list
    is fine for users with just the auto-created default.
  - `POST /api/profiles` `{name, game_mode?}` → `201 {profile}` (auth required).
- `/api/config` response gains: `auth_required: bool` (`mode != local`),
  `registration_open: bool` (true for now; M8 may gate), and keep existing
  fields. Bump `PublicConfig` struct + tests.
- Existing save endpoints (slots/caption/completion) unchanged except policy;
  their existing tests must keep passing.

### 4.7 Frontend (no new npm packages)

- `frontend/src/lib/api.ts`: types `User`, `Profile`, `ProfileInput`, config
  additions; functions `getMe()` (tolerates 401 → null), `login()`,
  `register()`, `logout()`, `getProfiles()`, `createProfile()`. All requests
  go through `fetch` (same-origin cookies are sent by default).
- `frontend/src/App.svelte`: add `user` state; on mount after `getConfig()`
  call `getMe()`; derived `authRequired = config.auth_required && user === null`
  → render an `Auth.svelte` login/register view **instead of the app shell**
  (needed for LAN/hub onboarding); sidebar footer shows `user.display_name` +
  a Logout button when authenticated; after logout clear `user` and re-run
  data loads (slots may have been hidden in lan/hub before login).
- `frontend/src/pages/Auth.svelte`: centered card, two tabs (Sign in / Create
  account), fields username/password/(display name for register), inline error
  text from API error messages, submit disabled while pending. Dark, Sheikah
  aesthetic consistent with `app.css`.
- Keep `Setup` page as-is; optionally surface `auth_required` in it (nice,
  not required).
- svelte-check must pass; mobile-first layout.

## 5. API contract summary (v0.5.0)

| Endpoint | Method | Scope policy | Response |
| --- | --- | --- | --- |
| `/api/auth/register` | POST | public | 201 `{user}` (+Set-Cookie) / 400 / 409 / 500 |
| `/api/auth/login` | POST | public | 200 `{user}` / 401 generic / 500 |
| `/api/auth/logout` | POST | public | 204 |
| `/api/me` | GET | auth in all modes | 200 `{user}` / 401 |
| `/api/profiles` | GET/POST | auth in all modes | 200/201 / 401 / 400 |
| `/api/slots`, `/api/slots/:n`, `/api/slots/:n/caption`, `/api/completion` | GET | open in local; 401 in lan/hub | unchanged otherwise |
| `/api/config` | GET | public | adds `auth_required`, `registration_open` |
| `/api/health`, `/api/map-objects` | GET | public | unchanged |

`user = { id, username, display_name, role, created_at }` — never sends
`password_hash`.

## 6. Tests (server crate, `#[tokio::test]` + `tempfile` sqlite)

1. Migration sanity: run migrations on temp file DB twice; tables/indexes exist.
2. Session store unit tests (CRUD/update/delete/expiry/purge) per §4.5.
3. Auth flow integration (on one shared tempfile DB per test):
   register → me → logout → me 401; register → login → me; login wrong
   password → generic 401; duplicate username → 409; invalid payloads → 400;
   first user admin, second user user.
4. Mode policy matrix: local mode — `/api/slots` works anonymously; lan + hub
   — `401` anonymous and `200` with a logged-in cookie jar (use the router
   directly with cookie extraction like existing tests do; or spawn server via
   `axum::serve` on an ephemeral port if easier for cookies).
5. Profiles: list contains auto-created Default; create works; unauthenticated
   → 401 even in local mode.
6. Existing suites (config, http, save-parser tests) stay green.

CI-relevant: no DB needed at compile time (runtime queries), CI unchanged.

## 7. Documentation

- `README.md`: documentation updates for auth — new env var row
  `WAYFINDER_COOKIE_SECURE`, note about `WAYFINDER_BIND` mode default, and a
  short "Authentication" section with the mode rules + hub-behind-TLS note.
- `CHANGELOG.md`: `[Unreleased]` → `### Added` entries (database + migrations,
  users/profiles/sessions, auth endpoints, custom sqlx-0.9 session store and
  *why*, frontend auth view, mode-aware bind default). Do **not** bump the
  workspace version (0.4.0 stays; next release cuts 0.5.0).
- `docs/PLAN.md`: tick the four M3 checkboxes in §8. Optional: paragraph in
  §5.3/§7 noting the session-store implementation detail (custom sqlx-0.9
  store, official crate blocked upstream — cross-reference the versions).
- `THIRD_PARTY_NOTICES.md`: no change expected (all MIT).

## 8. Commit plan (conventional commits, small + focused)

1. `feat(server): add sqlite database with embedded migrations`
2. `feat(server): argon2id user store and credentials backend`
3. `feat(server): session persistence via custom sqlx store`
4. `feat(server): auth endpoints and per-mode access policy`
5. `feat(frontend): login, registration, and logout flow`
6. `test(server): auth and policy integration tests` (or fold into 1–4)
7. `docs: document M3 database and auth`

Feature branch: `git switch develop && git pull && git switch -c
feat/m3-database-auth` (do all work on the feature branch, NOT develop).

## 9. Verification checklist (must all be green)

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm --prefix frontend install                      # lockfile refresh if needed
npm --prefix frontend run check
npm --prefix frontend run build
node --check scripts/fetch-map-assets.mjs          # untouched, belt+suspenders
```

Manual smoke test:
```sh
./scripts/fetch-map-assets.sh
npm --prefix frontend run build
WAYFINDER_MODE=local cargo run -p wayfinder-server    # anonymous browsing works
WAYFINDER_MODE=lan WAYFINDER_DATA_DIR=$(mktemp -d) WAYFINDER_BIND=127.0.0.1:18080 \
  cargo run -p wayfinder-server                       # register → 401 without login
# then: curl -i localhost:18080/api/config ; register+login via curl with cookie jar
```

Also confirm a fresh `data/` dir is created with `wayfinder.sqlite3` and
WAL/foreign_keys pragmas active, and `.gitignore` already ignores `data/`
(if not, add it).

## 10. Deferred to later milestones (don't sneak in)

- Rate limiting + origin/CSRF checks, upload validation, agent tokens → M8/M7.
- `PATCH/DELETE /api/profiles`, save upload/import APIs → M4.
- Password reset, email, display-name changes → post-v1.
- `save_slots`/`snapshots`/`agent_tokens`/`live_sessions`/`global_stats`
  tables → M4/M7 migrations (NOT in migration 0001).
- Final security hardening pass (rate limiting, origin checks, release build)
  → M8.

## 11. Pitfalls to verify against docs while implementing

- **tower-sessions 0.14 exact API**: trait `SessionStore` method signatures,
  `SessionRecord`/`SessionData` serialization granularity, `Expiry` enum,
  `SessionManagerLayer` builder methods (`with_secure`, `with_same_site_policy`,
  `with_expiry`) — confirm from docs.rs at code time; plan states intent only.
  Default cookie `Secure` is **true** — explicitly set to config value first.
- **axum-login 0.18**: use `AuthManagerLayerBuilder`; extractor
  `AuthSession<Backend>`; the `login_required!` macro **redirects** and is not
  used — write a custom `from_fn` guard for JSON 401s. Upstream 0.18 already
  removed `mut` on `AuthSession::{login, logout}` (commit “remove `mut` in
  AuthSession::{logout, login}”) — verify at code time.
- **sqlx 0.9**: runtime feature names changed (combined rt+tls features
  removed); `Migrate` trait API churn is irrelevant if using `sqlx::migrate!`
  wrapper; `query()`/`query_as()` are the API; `SqliteConnectOptions` +
  `busy_timeout` value (set ~5s) to avoid `database is locked` under WAL from
  pooled writers; keep a small pool (≤ 5) since SQLite writes serialize.
- **spawn_blocking hygiene**: Argon2 hash/verify only; ordinary queries stay
  on async executor.
- Keep failure messages generic; never log password material or PHC hashes.
- `time` crate feature set minimal; `SqlValue`/`FromRow` decoders for INTEGER
  unix seconds are manual (i64) — no chrono dependency needed.
- End of file: check `main.rs` logs migration + auth status lines once
  (e.g., `users: <n>` — no PII).
