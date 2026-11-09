# Wayfinder Slate — M5: Dashboard and Map (remaining) — Implementation Plan

> Written: 2026-11-09
> Target repo: `/workspace/wayfinder-slate`
> Branch: `feat/m5-map-completion` created from `develop`
> Self-contained hand-off plan for a fresh build session. Decisions below are
> settled — implement them; do not re-litigate.

## 0. Orientation (read first)

- `/workspace` is **not** the repo root. The repo lives entirely in
  `/workspace/wayfinder-slate`. Read `/workspace/INDEX.md` for orientation.
- `/workspace/maps/` (zeldadungeon/maps clone) and `/workspace/save/` (real BotW
  save sample, personal data) are **read-only reference**. Never modify, never
  commit, never redistribute the save.
- Read `README.md`, `CONTRIBUTING.md`, and `docs/PLAN.md` (esp. §5.2, §6.2–6.4,
  §7.5, §8, §10, §11) before writing code. `docs/m3-plan.md` is the format model
  for this document and shows the established workflow.
- Milestone to finish (`docs/PLAN.md` §8, M5 — only these three boxes are open):
  - [ ] Divine Beast layers (with completion)
  - [ ] Marker clustering, filters, progress
  - [ ] Coordinate calibration tests
- Tick those boxes in `docs/PLAN.md` when done. M0–M4 are **not** in scope;
  save ingestion/snapshots (M4) and live mode (M7) stay deferred.

## 1. Environment facts

- Passwordless-sudo Arch container. Toolchain: `rustc`/`cargo` 1.99.x,
  `node` 26.13.0, `npm` 12.2.2. `frontend/node_modules` is installed.
- Map assets are fetched by `./scripts/fetch-map-assets.sh` (~49 MB, gitignored)
  into `frontend/static/map/botw/`. The dataset generator needs them; `main.ts`
  already imports Leaflet + markercluster CSS.
- The dev save is referenced with `WAYFINDER_TEST_SAVE=/workspace/save/save_0/0/game_data.sav`
  (never committed).
- All commands in §9 must pass before the work is considered complete.

## 2. Current state (already done in M5)

- `frontend/src/pages/MapView.svelte` renders Leaflet tiles plus plain
  `L.marker`s in `L.layerGroup`s for shrines, towers, memories, koroks,
  locations, enemies, and treasures. Layer toggles exist; completion is a
  panel of **counts** only.
- `frontend/src/lib/coords.ts` owns the transform: `lat = -z`, `lng = x`,
  custom CRS with `scale = 750/12000`, `offset = 375`, `MAX_ZOOM = 5`.
- `crates/wayfinder-save` computes `CompletionSummary` (shrines, towers, koroks,
  memories, locations, enemies, treasures) and a real-save integration test.
- `frontend/static/data/map_objects.json` (version **2**) joins ZD markers to
  save flags for all types except Divine Beasts; `scripts/validate-map-objects.mjs`
  enforces invariants.
- `leaflet.markercluster` is already in `frontend/package.json`; its two CSS
  files are already imported in `frontend/src/main.ts`. The plugin's **JS** is
  not imported yet and the plugin is unused.
- There is **no** JS test runner yet (only `svelte-check` + `vite build`).

## 3. Settled decisions

### 3.1 Divine Beast completion flags (verified)

The four beasts are tracked by CRC32-confirmed save flags (source:
`marcrobledo/savegame-editors` `zelda-botw.hashes.csv`, MIT). All four records
are present (value `0` = not yet freed) in every slot of the development save.

| Beast | Flag | CRC32 |
| --- | --- | --- |
| Vah Ruta | `Clear_RemainsWater` | `0xb742dd6c` (3074612588) |
| Vah Rudania | `Clear_RemainsFire` | `0x4007e1ee` (1074258414) |
| Vah Medoh | `Clear_RemainsWind` | `0x39d2bcdf` (970112223) |
| Vah Naboris | `Clear_RemainsElectric` | `0x18d100e9` (416350441) |

Total is **4**. The development save has `0/4` freed, so the real-save test
asserts `completed == 0`.

### 3.2 Divine Beast coordinates (verified)

Coordinates come from the **Main Quest** layer of the pinned upstream
`public/botw/markers/wiki.json`. The dataset stores game coordinates; convert
with the same transform the treasure generator documents:

```
x = mapY / 2
z = -mapX / 2
```

| Marker (wiki.json) | mapX, mapY | dataset x, z |
| --- | --- | --- |
| Divine Beast Vah Ruta | `1101, 6675` | `3337.5, -550.5` |
| Divine Beast Vah Rudania | `4900, 3420` | `1710, -2450` |
| Divine Beast Vah Naboris | `-5930, -7769` | `-3884.5, 2965` |
| Divine Beast Vah Medoh | `3662, -7243` | `-3621.5, -1831` |

Cross-checks with the existing dataset confirm the transform: Rudania lands on
Goron City, Naboris on Gerudo Town, Medoh on Rito Village, and Hyrule Castle's
upstream pin `[1223, -508]` maps to the dataset's `x=-254, z≈-611.4`.

Add a `divineBeasts` array to `map_objects.json` with objects shaped
`{ "flag": "Clear_Remains…", "name": "Divine Beast Vah …", "x": …, "z": … }`,
and bump the dataset `version` from `2` to `3`. Source attribution:
`"Zelda Dungeon wiki markers (Main Quest layer) + marcrobledo/savegame-editors (MIT) Clear_Remains* flags"`.

### 3.3 Icon: reuse `mainquest.png`

Upstream has **no** dedicated BotW Divine Beast icon. The beast markers live in
the Main Quest layer, which uses `icons/mainquest.png` (the Hylian crest). That
icon is already in `scripts/map-assets.sha256` and already fetched. Use
`/map/botw/icons/mainquest.png` at roughly `34×20` (upstream native size is
`50×30`; scale down for a map pin). **Do not** add a new asset or touch the
fetch manifest or `THIRD_PARTY_NOTICES.md` for this milestone.

### 3.4 Per-object completion via a new endpoint

Filters and "nearest unfinished" need per-object completion, not counts. Add
`GET /api/slots/{slot}/map` (already in PLAN §7.5). The server joins the parsed
save against the embedded `data/map_objects.json` and returns the identifiers of
completed objects, keyed by the dataset's own lookup field:

- shrines/towers/memories/locations/enemies/koroks/divine beasts → the `flag`
  name (koroks already carry `MainField_Npc_HiddenKorok*` flags in the dataset);
- treasure chests → the chest's CRC32 flag hash (see §3.5).

Use per-object flags for the map endpoint. Do **not** change
`wayfinder-save::SaveFile::completion_summary()`'s korok counting (it uses the
`HiddenKorok_Number` counter and the dataset flag count in the development save
matches it exactly at 72). The map view's progress should be derived from the
per-object response, not from `CompletionSummary`, so the two can never disagree
on screen.

### 3.5 Checkbox: chest hashes in the dataset

The dataset's treasure entries currently carry `id` (e.g. `Treasure0001`) but
not the save hash. Add `"hash": <u32>` to every treasure in
`scripts/build-treasure-objects.mjs` (it already builds `hashByMarkerId`). Then
the map endpoint can return set hashes and the frontend can match
`object.hash` directly. JSON numbers up to `2^32-1` are safe in JS.

Regenerate the dataset with the generator (needs fetched assets; see §9), and
extend `scripts/validate-map-objects.mjs` to require a non-empty, unique
`hash` per treasure and to cross-check the dataset hashes against
`scripts/treasure-flags.json` and `crates/wayfinder-save/src/treasure_hashes.rs`
(the existing id checks stay).

### 3.6 Clustering approach

Use `leaflet.markercluster` (already a dependency; CSS already imported). Import
the plugin's JS (`import 'leaflet.markercluster';`) so `L.markerClusterGroup`
registers. Cluster the dense layers — koroks, chests, enemies, locations (and
memories if desired) — with `maxClusterRadius` ≈ 60,
`showCoverageOnHover: false`, `spiderfyOnMaxZoom: true`, and a sensible
`disableClusteringAtZoom` (start at 4, tune visually). Keep shrines, towers, and
Divine Beasts as individual pins. Store cluster groups in the existing `groups`
map (type it as `L.LayerGroup | L.MarkerClusterGroup`) so the toggle logic keeps
working unchanged.

### 3.7 Filters, progress, "nearest unfinished"

- Completion filter is a 3-state segmented control in the map panel:
  **All** / **Unfinished** (hide completed) / **Completed** (only completed).
  Applying a filter rebuilds the visible markers (clear + re-add matching items);
  acceptable at ~2k markers.
- Progress: per-layer progress bars in the panel plus an overall completion
  percentage, all derived from the per-object response and dataset totals.
- "Nearest unfinished": button that picks the nearest unfinished object among
  currently visible types, using the player's last-saved position
  (`summary.stats.location.x/z`) as the origin when available, otherwise the map
  centre; then `flyTo` and open its popup.
- Put the pure logic (`isComplete`, distance/nearest, filtering) in a small
  `frontend/src/lib/mapObjects.ts` module so it is unit-testable without Svelte.

### 3.8 Coordinate calibration harness

Introduce **vitest + jsdom** (no existing JS runner). Add
`frontend/vitest.config.ts` with `environment: 'jsdom'`, a `"test": "vitest run"`
script, and wire `npm run test` into CI and `CONTRIBUTING.md`.

The CRS maths are verified against the installed Leaflet source:
`CRS.Simple.scale(zoom) = 2^zoom`, and `createBotwCrs()` does not override
`scale`, only `transformation`. Therefore at `MAX_ZOOM = 5`:

```
pixelX = 2 * x + 12000
pixelY = 2 * z + 12000
```

Calibration tests must use **independent upstream marker coordinates**, not the
dataset's derived values. Pin this table (tolerance ±2 game units / ±4 px to
absorb the marker positional noise):

| Landmark | upstream mapX, mapY | expected x, z | expected pixel @ z5 |
| --- | --- | --- | --- |
| Hyrule Castle (`pins.json`) | `1223, -508` | `-254, -611.5` | `11492, 10777` |
| Akkala Tower (`pins.json`) | `3000, 6616` | `3308, -1500` | `18616, 9000` |
| Gerudo Tower (`pins.json`) | `-3657, -7332` | `-3666, 1828.5` | `4668, 15657` |
| Vah Ruta (`wiki.json`) | `1101, 6675` | `3337.5, -550.5` | `18675, 10899` |
| Vah Naboris (`wiki.json`) | `-5930, -7769` | `-3884.5, 2965` | `4231, 17930` |

## 4. Work breakdown

### 4.1 `crates/wayfinder-save`

- `src/lib.rs`:
  - add `pub const DIVINE_BEAST_TOTAL: usize = 4;`
  - add `pub divine_beasts: CompletionCount` to `CompletionSummary`;
  - compute it from the four `Clear_Remains*` flags via `is_flag_set` (mirror
    the shrine/tower loops; a small `const` slice of the four names is fine);
  - add a unit test: a synthetic save with one `Clear_RemainsWater` record set
    yields `completed == 1`, total `4`.
- `tests/real_save.rs`: assert `summary.divine_beasts.total == 4` and
  `summary.divine_beasts.completed == 0`.
- No changes to `keys.rs` unless a `const` slice is preferred there.
- Keep all existing completion tests/value assertions unchanged.

### 4.2 Dataset, generator, validator

- `scripts/build-treasure-objects.mjs`: add `hash` to each treasure entry
  (`hash` from the existing `flagEntries`/`hashByMarkerId`); keep deterministic
  ordering. Run it after fetching assets.
- Add `scripts/build-divine-beasts.mjs` (new; dependency-free Node, same style):
  read `frontend/static/map/botw/markers/wiki.json`, pick the four
  `"Divine Beast Vah *"` markers whose `id` has no extra suffix, apply
  `x = mapY/2, z = -mapX/2`, and write `dataset.divineBeasts` + the
  `source.divineBeasts` attribution. Set `dataset.version = 3`. Validate
  finiteness/±6000 and exactly 4 entries. (It is fine for this script and the
  treasure script to both write `version = 3`; run them in a documented order
  and commit the single resulting JSON.)
- `scripts/validate-map-objects.mjs`:
  - accept `dataset.version === 3`;
  - add `divineBeasts: 4` to `EXPECTED_TOTALS`;
  - add `EXPECTED_FLAGS.divineBeasts = ['Clear_RemainsWater','Clear_RemainsFire','Clear_RemainsWind','Clear_RemainsElectric']`;
  - require non-empty `name` for divine beasts;
  - add the treasure `hash` checks described in §3.5.
- Commit the regenerated `frontend/static/data/map_objects.json`
  (now version 3, `divineBeasts`, treasure `hash`).

### 4.3 Server endpoint (`crates/wayfinder-server`)

- `src/saves.rs`:
  - Extend the dataset loader (currently `MapObjectLabels`) into a
    `MapObjects` struct that keeps the existing `labels` behaviour (so slot
    labels/tests still work) and also stores, per type, the dataset `flag`
    strings, plus the treasure `hash` values. Parse tolerantly with serde
    defaults; keep `from_json` fallible.
  - Add `pub fn load_map_completion(config: &Config, slot: usize, objects: &MapObjects) -> Result<MapCompletion, SlotError>`
    that parses the save (reuse `parse_save`), builds `CompletedObjects` by
    testing `save.is_flag_set(flag)` per type and `save.is_key_set(hash)` for
    chests, and returns only the completed identifiers.
  - Response types (serde `Serialize`):
    ```rust
    pub struct MapCompletion { pub slot: usize, pub completed: CompletedObjects }
    #[derive(Default)]
    pub struct CompletedObjects {
        pub shrines: Vec<String>,
        pub towers: Vec<String>,
        pub memories: Vec<String>,
        pub koroks: Vec<String>,
        pub locations: Vec<String>,
        pub enemies: Vec<String>,
        pub divine_beasts: Vec<String>,
        pub treasures: Vec<u32>,
    }
    ```
- `src/http.rs`:
  - Route `GET /api/slots/{slot}/map` inside the existing `save_routes` group so
    it inherits the local-open / LAN-hub-auth policy.
  - Reuse the startup-cached dataset, renamed appropriately
    (`map_objects()` → `&'static saves::MapObjects`), and keep the existing
    `slot_error_response` mapping (404 / 422).
  - `no_store(Json(...))` like the sibling slot handlers.
- Tests: temp save dir + synthetic save containing e.g. `Clear_Dungeon015`,
  `MapTower_07`, one korok flag, one `Clear_RemainsWater`, and a chest hash;
  assert the response lists exactly those identifiers. Add a LAN-mode
  unauthenticated `401` case mirroring the existing policy tests.

### 4.4 Frontend API/types (`frontend/src/lib/api.ts`)

- Add `DivineBeast` to `MapObjectType`, add `divineBeasts: MapObject[]` to
  `MapObjectsDataset`, add `hash?: number` to `MapObject`, and add
  `divine_beasts: CompletionCount` to `CompletionSummary`.
- Add `CompletedObjects` / `MapCompletion` interfaces and
  `getMapCompletion(slot: number): Promise<MapCompletion>` hitting
  `/api/slots/{slot}/map` (handle 404/422 like `getSlot`).

### 4.5 Map view (`frontend/src/pages/MapView.svelte` + new `lib/mapObjects.ts`)

- New props: `slot: number | null` and
  `position: { x: number; z: number } | null` (from the selected slot's stats).
- On mount / when `slot` changes, fetch `getMapCompletion(slot)` into
  `$state`; guard against stale responses (request counter, like `App.svelte`).
- Extend `OBJECT_TYPES`, `STYLES`, visibility state, counts, and toggles with
  `divineBeast` (icon `mainquest`, colour e.g. `#d98cff`, label
  "Divine Beasts"). Add the Divine Beasts row to the completion panel.
- Clustering per §3.6; filters + progress + "nearest unfinished" per §3.7.
- New `lib/mapObjects.ts` exports the pure helpers:
  `isComplete(type, object, completed)`, `filterObjects(type, objects, completed, filter)`,
  and `nearestUnfinished(type, objects, completed, origin)`.
- Keep the existing mobile-first CSS/layout and accessibility (buttons/labels,
  keyboard-usable controls).

### 4.6 App plumbing (`frontend/src/App.svelte`)

- Pass `slot={selectedSlot}` and
  `position={summary?.stats.location ? { x: summary.stats.location.x, z: summary.stats.location.z } : null}`
  to `<MapView>`.

### 4.7 Tests

- `frontend/src/lib/coords.test.ts` (vitest + jsdom):
  - `gameToLatLng(x, z)` equals `[-z, x]`;
  - `createBotwCrs().latLngToPoint(gameToLatLng(x, z), 5)` equals
    `L.point(2*x + 12000, 2*z + 12000)` across the §3.8 table (tolerance);
  - world corners: `gameToLatLng(-6000, -6000)` → pixel `(0, 0)`;
    `gameToLatLng(6000, 6000)` → pixel `(24000, 24000)`;
  - `botwBounds` spans the full tile pyramid and round-trips through
    `latLngToPoint`/`pointToLatLng`.
- `frontend/src/lib/mapObjects.test.ts`: completion matching for a named flag,
  a korok flag, and a chest hash; filter modes; nearest-unfinished selection and
  its empty-list fallback.
- Rust tests per §4.1 and §4.3.

### 4.8 Tooling, CI, docs

- `frontend/package.json`: add `vitest` + `jsdom` devDependencies and
  `"test": "vitest run"`.
- `.github/workflows/ci.yml` (frontend job): add `npm run test` after
  `npm run check` (keep `node --check ../scripts/*.mjs` and the dataset
  validator).
- `CONTRIBUTING.md`: add `npm --prefix frontend run test` to the checks list.
- `scripts/validate-map-objects.mjs` remains runnable in CI (assets not needed;
  it reads the committed dataset).

## 5. API contract summary (additions only)

| Endpoint | Method | Scope policy | Response |
| --- | --- | --- | --- |
| `/api/slots/{slot}/map` | GET | open in local; auth in lan/hub (same as `/api/slots*`) | `200 { slot, completed: { … } }` / `404` / `422` |

`completed` fields: `shrines`, `towers`, `memories`, `koroks`, `locations`,
`enemies`, `divine_beasts` → arrays of dataset `flag` strings; `treasures` →
array of CRC32 hashes (`number`). Only completed identifiers are present.

`CompletionSummary` (served by `/api/slots/{slot}` and `/api/completion`) gains
`divine_beasts: { total, completed, percentage }`.

## 6. Tests (summary)

1. Rust: Divine Beast summary unit test; real-save Divine Beast assertion;
   map-completion endpoint happy path + policy `401`.
2. Frontend: coordinate calibration table + corner/round-trip; `mapObjects`
   completion/filter/nearest helpers.
3. Existing suites: save parser, server config/http/auth, `svelte-check`,
   `validate-map-objects.mjs` all stay green.
4. Manual smoke test: map with Divine Beasts toggled; filter states; nearest
   unfinished; clustering at low zoom; mobile layout.

## 7. Documentation

- `docs/PLAN.md`: tick the three M5 boxes; if helpful, note the new
  `/api/slots/{slot}/map` endpoint and the `Clear_Remains*` flags in §6.4.
- `CHANGELOG.md` `[Unreleased]` → `### Added`: Divine Beast layer/completion,
  per-object map completion endpoint, marker clustering + completion filters +
  progress + nearest-unfinished, coordinate calibration tests.
- `README.md`: optional one-line mention of the Divine Beasts layer.
- `THIRD_PARTY_NOTICES.md`: **no change** (reusing the already-attributed
  `mainquest.png`).
- `CONTRIBUTING.md`: add the frontend test command.

## 8. Commit plan (conventional commits, small + focused)

1. `feat(save): track Divine Beast completion`
2. `feat(map): add Divine Beast dataset and layer`
3. `feat(server): per-object map completion endpoint`
4. `feat(map): cluster markers with completion filters and progress`
5. `test(frontend): add coordinate calibration tests`
6. `docs: document M5 completion`

Create the feature branch first:
`git switch develop && git pull && git switch -c feat/m5-map-completion`.
Do all work on the feature branch, not `develop`.

## 9. Verification checklist (must all be green)

```sh
./scripts/fetch-map-assets.sh                 # required to regenerate the dataset
node scripts/build-treasure-objects.mjs       # adds treasure `hash` + regenerates treasure_hashes.rs
node scripts/build-divine-beasts.mjs          # adds divineBeasts + bumps version to 3
node scripts/validate-map-objects.mjs         # dataset invariants

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

npm --prefix frontend install                 # picks up vitest + jsdom
npm --prefix frontend run check
npm --prefix frontend run test
npm --prefix frontend run build
node --check scripts/build-divine-beasts.mjs
node --check scripts/build-treasure-objects.mjs
```

Real-save check (private; do not commit):

```sh
WAYFINDER_TEST_SAVE=/workspace/save/save_0/0/game_data.sav \
  cargo test -p wayfinder-save --test real_save
```

Manual smoke test:

```sh
npm --prefix frontend run build
WAYFINDER_MODE=local cargo run -p wayfinder-server
# open http://127.0.0.1:8080 → Hyrule Map:
#  - Divine Beasts layer toggles, 0/4 in the panel (dev save)
#  - clustering on Koroks/Chests/Enemies/Locations at low zoom
#  - All / Unfinished / Completed filter states
#  - "Nearest unfinished" flies to a marker
```

## 10. Deferred / out of scope (do not sneak in)

- M4 save ingestion: folder watcher, snapshots, uploads, `import-path`,
  `save_slots`/`snapshots` tables.
- M6 progress-over-time charts, medals/titles, global opt-in aggregates.
- M7 live telemetry (`wayfinder-control` polling, SSE, agent).
- Per-beast interior/dungeon detail, DLC "Final Trial", or quest layers.
- A custom Divine Beast icon (option A was chosen; revisit later if desired).
- Reworking `completion_summary()`'s korok counter.

## 11. Pitfalls to verify at code time

- **Leaflet CRS scale:** `CRS.Simple.scale(zoom) = 2^zoom` (confirmed in
  `frontend/node_modules/leaflet/dist/leaflet-src.js`), so the zoom-5 pixel
  formula is `2*coord + 12000`. If `coords.ts` ever overrides `scale`, update
  the calibration tests and formula together.
- **markercluster registration:** importing only the CSS is not enough; import
  `leaflet.markercluster` for JS. `@types/leaflet.markercluster` is already a
  devDependency.
- **Type unions:** adding `divine_beast`/`divineBeasts` touches every exhaustive
  switch in `MapView.svelte` (`STYLES`, `getVisibility`, `markerName`,
  `datasetMap`) — TypeScript's `noUnusedLocals`/`strict` will catch omissions.
- **Dataset order of operations:** both generators write the dataset; run the
  treasure generator first (it currently hard-sets `version = 2` — change it to
  `3` when adding `hash`), then the Divine Beast generator, then the validator.
  Commit the single final JSON.
- **Treasure hash cross-checks:** keep the validator's existing comparison
  between `treasure-flags.json` and `treasure_hashes.rs`; add the dataset `hash`
  set to that check so all three agree.
- **`spawn_blocking`:** the save parse in the map endpoint is the same as the
  existing slot handlers. If this is extended to large saves, keep parsing off
  the async runtime per CONTRIBUTING.
- **No PII in logs:** never log save contents, positions, or chest hashes at
  info level.
- **Stale fetch guard:** the map completion request must ignore responses for a
  slot that is no longer selected (slot switching is fast in the toolbar).