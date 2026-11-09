#!/usr/bin/env node
/**
 * Build the Divine Beast section of the map-object dataset.
 *
 * The four Divine Beasts are pinned to the Main Quest markers in the upstream
 * Zelda Dungeon `wiki.json` and to the CRC32-confirmed `Clear_Remains*` save
 * flags (marcrobledo/savegame-editors, MIT). Coordinates are converted from
 * Zelda Dungeon map space to game x/z with the same transform the treasure
 * generator uses: `x = mapY / 2`, `z = -mapX / 2`.
 *
 * Inputs:
 *   - `frontend/static/map/botw/markers/wiki.json` — the upstream Zelda Dungeon
 *     marker document, fetched by `scripts/fetch-map-assets.sh` at the pinned
 *     commit.
 *
 * Outputs:
 *   - the `divineBeasts` array and `source.divineBeasts` entry in the dataset,
 *     and the `version` bump to 3.
 *
 * Run with `node scripts/build-divine-beasts.mjs` after fetching the map
 * assets and after `build-treasure-objects.mjs` (both write the shared dataset).
 * The output is committed, so this only needs to run when the pinned upstream
 * revision or the flag mapping changes.
 */
import { readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const UPSTREAM_PATH = path.join(
  ROOT,
  'frontend',
  'static',
  'map',
  'botw',
  'markers',
  'wiki.json',
);
const DATASET_PATH = path.join(ROOT, 'frontend', 'static', 'data', 'map_objects.json');

/** The dataset stores game coordinates; the validator rejects anything wider. */
const COORDINATE_LIMIT = 6000;
/** Expected number of Divine Beasts. */
const EXPECTED_BEASTS = 4;

const SOURCE_ATTRIBUTION =
  'Zelda Dungeon wiki markers (Main Quest layer) + marcrobledo/savegame-editors (MIT) Clear_Remains* flags';

/**
 * Upstream Main Quest marker id -> save flag, in the order the save parser
 * reads them. The plain marker ids carry no quest-objective suffix.
 */
const BEASTS = [
  ['Divine Beast Vah Ruta', 'Clear_RemainsWater'],
  ['Divine Beast Vah Rudania', 'Clear_RemainsFire'],
  ['Divine Beast Vah Medoh', 'Clear_RemainsWind'],
  ['Divine Beast Vah Naboris', 'Clear_RemainsElectric'],
];

async function readJson(file) {
  return JSON.parse(await readFile(file, 'utf8'));
}

/**
 * Flatten the upstream layer-group document into marker entries.
 * Each layer holds one or more sub-layers, each with its own marker list.
 */
function flattenUpstreamLayers(document) {
  const markers = [];
  for (const layer of Object.values(document)) {
    if (!layer || !Array.isArray(layer.layers)) continue;
    for (const subLayer of layer.layers) {
      for (const marker of subLayer.markers ?? []) {
        markers.push({ ...marker, group: layer.name });
      }
    }
  }
  return markers;
}

/** Convert a Zelda Dungeon map coordinate to the dataset's game x/z. */
function toGameCoordinates([mapX, mapY]) {
  return { x: mapY / 2, z: -mapX / 2 };
}

async function main() {
  const upstream = await readJson(UPSTREAM_PATH);
  const byId = new Map(flattenUpstreamLayers(upstream).map((marker) => [marker.id, marker]));

  const divineBeasts = BEASTS.map(([id, flag]) => {
    const marker = byId.get(id);
    if (!marker) {
      throw new Error(`missing upstream Main Quest marker: ${id}`);
    }
    if (!Array.isArray(marker.coords) || marker.coords.length !== 2) {
      throw new Error(`${id}: expected a [mapX, mapY] coordinate pair`);
    }
    const { x, z } = toGameCoordinates(marker.coords);
    if (!Number.isFinite(x) || !Number.isFinite(z)) {
      throw new Error(`${id}: non-finite coordinate`);
    }
    if (Math.abs(x) > COORDINATE_LIMIT || Math.abs(z) > COORDINATE_LIMIT) {
      throw new Error(`${id}: coordinate outside the game world`);
    }
    return { flag, name: id, x, z };
  });

  if (divineBeasts.length !== EXPECTED_BEASTS) {
    throw new Error(`expected ${EXPECTED_BEASTS} Divine Beasts, built ${divineBeasts.length}`);
  }

  const dataset = await readJson(DATASET_PATH);
  dataset.version = 3;
  dataset.source = { ...dataset.source, divineBeasts: SOURCE_ATTRIBUTION };
  dataset.divineBeasts = divineBeasts;

  await writeFile(DATASET_PATH, `${JSON.stringify(dataset)}\n`);

  console.log(
    `Wrote ${divineBeasts.length} Divine Beasts to ${path.relative(ROOT, DATASET_PATH)}.`,
  );
}

main().catch((error) => {
  console.error(`\nerror: ${error.message}`);
  process.exitCode = 1;
});
