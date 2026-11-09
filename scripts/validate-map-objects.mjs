#!/usr/bin/env node
/**
 * Validate the committed map object dataset used for completion tracking.
 *
 * The dataset joins Zelda Dungeon marker coordinates with save flag names. A
 * malformed entry breaks marker rendering or makes 100% completion impossible,
 * so check the invariants the Rust and Svelte code rely on:
 *
 * - every expected object type is present with the documented total;
 * - shrine, tower, and memory flags follow the sequences the save parser reads;
 * - flags are unique within each type;
 * - coordinates are finite and inside the Breath of the Wild world;
 * - each type carries the fields used to label its markers;
 * - the treasure chests match the committed chest flag table, and the three
 *   gambling mini-game chests that have no save flag are absent.
 *
 * Run with `node scripts/validate-map-objects.mjs`.
 */
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const DATASET_PATH = path.join(ROOT, 'frontend/static/data/map_objects.json');
const TREASURE_FLAGS_PATH = path.join(ROOT, 'scripts/treasure-flags.json');
const TREASURE_HASHES_PATH = path.join(ROOT, 'crates/wayfinder-save/src/treasure_hashes.rs');

const EXPECTED_TOTALS = {
  shrines: 136,
  towers: 15,
  koroks: 900,
  memories: 14,
  locations: 187,
  enemies: 84,
  divineBeasts: 4,
};

/**
 * Treasure chests are joined by their save-flag hash rather than by flag name,
 * so they are checked separately. The count is the 1315 static chest placements
 * minus the three gambling mini-game chests, which have no save flag.
 */
const EXPECTED_TREASURES = 1312;
const GAMBLE_CHESTS = ['Treasure0055', 'Treasure0056', 'Treasure0057'];

const COORDINATE_LIMIT = 6000;

const EXPECTED_FLAGS = {
  shrines: sequence('Clear_Dungeon', 0, 136, 3),
  towers: sequence('MapTower_', 1, 15, 2),
  memories: sequence('IsGet_MemoryPhoto_', 0, 14, 3),
  divineBeasts: [
    'Clear_RemainsWater',
    'Clear_RemainsFire',
    'Clear_RemainsWind',
    'Clear_RemainsElectric',
  ],
};

const failures = [];

function sequence(prefix, start, count, width) {
  return Array.from(
    { length: count },
    (_, index) => `${prefix}${String(start + index).padStart(width, '0')}`,
  );
}

function fail(message) {
  failures.push(message);
}

function isText(value) {
  return typeof value === 'string' && value.trim().length > 0;
}

function checkFlagSequence(type, objects) {
  const expected = EXPECTED_FLAGS[type];
  if (!expected) return;

  const actual = new Set(objects.map((object) => object.flag));
  const missing = expected.filter((flag) => !actual.has(flag));
  const extra = [...actual].filter((flag) => !expected.includes(flag));

  if (missing.length > 0) {
    fail(`${type}: missing ${missing.length} expected flag(s), first: ${missing[0]}`);
  }
  if (extra.length > 0) {
    fail(`${type}: ${extra.length} unexpected flag(s), first: ${extra[0]}`);
  }
}

function checkLabel(type, object, where) {
  if (type === 'shrines' || type === 'towers' || type === 'memories') {
    if (!isText(object.name)) fail(`${where}: expected a non-empty name`);
  } else if (type === 'koroks') {
    if (!isText(object.seed)) fail(`${where}: expected a non-empty seed`);
    if (!isText(object.loc)) fail(`${where}: expected a non-empty loc`);
  } else if (type === 'locations') {
    if (!isText(object.match_name)) fail(`${where}: expected a non-empty match_name`);
  } else if (type === 'enemies') {
    if (!isText(object.type)) fail(`${where}: expected a non-empty type`);
  } else if (type === 'divineBeasts') {
    if (!isText(object.name)) fail(`${where}: expected a non-empty name`);
  }
}

function checkCoordinates(object, where) {
  for (const axis of ['x', 'z']) {
    const value = object[axis];
    if (typeof value !== 'number' || !Number.isFinite(value)) {
      fail(`${where}: expected a finite ${axis} coordinate`);
    } else if (Math.abs(value) > COORDINATE_LIMIT) {
      fail(`${where}: ${axis}=${value} is outside ±${COORDINATE_LIMIT}`);
    }
  }
}

const text = await readFile(DATASET_PATH, 'utf8');
let dataset;
try {
  dataset = JSON.parse(text);
} catch (error) {
  console.error(`Could not parse ${DATASET_PATH}: ${error.message}`);
  process.exit(1);
}

if (dataset.version !== 3) {
  fail(`expected dataset version 3, found ${JSON.stringify(dataset.version)}`);
}

if (dataset.source === null || typeof dataset.source !== 'object') {
  fail('expected a source attribution object');
}

for (const [type, expectedTotal] of Object.entries(EXPECTED_TOTALS)) {
  const objects = dataset[type];
  if (!Array.isArray(objects)) {
    fail(`${type}: expected an array`);
    continue;
  }
  if (objects.length !== expectedTotal) {
    fail(`${type}: expected ${expectedTotal} objects, found ${objects.length}`);
  }

  const seenFlags = new Set();
  objects.forEach((object, index) => {
    const where = `${type}[${index}]`;
    if (object === null || typeof object !== 'object' || Array.isArray(object)) {
      fail(`${where}: expected an object`);
      return;
    }

    if (!isText(object.flag)) {
      fail(`${where}: expected a non-empty flag`);
    } else if (seenFlags.has(object.flag)) {
      fail(`${where}: duplicate flag ${object.flag}`);
    } else {
      seenFlags.add(object.flag);
    }

    checkCoordinates(object, where);

    checkLabel(type, object, where);
  });

  checkFlagSequence(type, objects);
}

// Treasure chests carry a marker id and a category instead of a flag name.
const treasures = dataset.treasures;
const treasureIds = new Set();
const treasureHashes = new Set();
if (!Array.isArray(treasures)) {
  fail('treasures: expected an array');
} else {
  if (treasures.length !== EXPECTED_TREASURES) {
    fail(`treasures: expected ${EXPECTED_TREASURES} objects, found ${treasures.length}`);
  }

  treasures.forEach((object, index) => {
    const where = `treasures[${index}]`;
    if (object === null || typeof object !== 'object' || Array.isArray(object)) {
      fail(`${where}: expected an object`);
      return;
    }

    if (!isText(object.id)) {
      fail(`${where}: expected a non-empty id`);
    } else if (treasureIds.has(object.id)) {
      fail(`${where}: duplicate id ${object.id}`);
    } else {
      treasureIds.add(object.id);
    }

    if (
      typeof object.hash !== 'number' ||
      !Number.isInteger(object.hash) ||
      object.hash <= 0 ||
      object.hash > 0xffffffff
    ) {
      fail(`${where}: expected a u32 save-flag hash`);
    } else if (treasureHashes.has(object.hash)) {
      fail(`${where}: duplicate hash ${object.hash}`);
    } else {
      treasureHashes.add(object.hash);
    }

    if (!isText(object.name)) fail(`${where}: expected a non-empty name`);
    if (!isText(object.group)) fail(`${where}: expected a non-empty group`);

    checkCoordinates(object, where);
  });

  for (const gamble of GAMBLE_CHESTS) {
    if (treasureIds.has(gamble)) {
      fail(`treasures: ${gamble} is a gambling chest and has no save flag`);
    }
  }

  // The dataset and the flag table are generated from the same source, so the
  // marker ids they carry must agree exactly.
  let flagDocument;
  try {
    flagDocument = JSON.parse(await readFile(TREASURE_FLAGS_PATH, 'utf8'));
  } catch (error) {
    fail(`could not read ${path.relative(ROOT, TREASURE_FLAGS_PATH)}: ${error.message}`);
  }
  if (flagDocument) {
    const flaggedIds = new Set(Object.values(flagDocument.treasure ?? {}));
    const missing = [...flaggedIds].filter((id) => !treasureIds.has(id));
    const extra = [...treasureIds].filter((id) => !flaggedIds.has(id));
    if (missing.length > 0) {
      fail(`treasures: ${missing.length} chest(s) missing from the dataset, first: ${missing[0]}`);
    }
    if (extra.length > 0) {
      fail(`treasures: ${extra.length} chest(s) not in the flag table, first: ${extra[0]}`);
    }

    const flagHashes = Object.keys(flagDocument.treasure ?? {}).map(Number);

    // Every chest carries the CRC32 hash of its save flag so the map view can
    // match completed chests. The dataset hashes must equal the flag table.
    const missingHashes = flagHashes.filter((hash) => !treasureHashes.has(hash));
    const extraHashes = [...treasureHashes].filter((hash) => !flagHashes.includes(hash));
    if (missingHashes.length > 0) {
      fail(
        `treasures: ${missingHashes.length} chest hash(es) missing from the dataset, ` +
          `first: ${missingHashes[0]}`,
      );
    }
    if (extraHashes.length > 0) {
      fail(
        `treasures: ${extraHashes.length} dataset hash(es) not in the flag table, ` +
          `first: ${extraHashes[0]}`,
      );
    }

    // The Rust save parser counts opened chests from a generated hash list, so
    // that list must cover exactly the hashes in the flag table.
    const rustPath = TREASURE_HASHES_PATH;
    let rustSource;
    try {
      rustSource = await readFile(rustPath, 'utf8');
    } catch (error) {
      fail(`could not read ${path.relative(ROOT, rustPath)}: ${error.message}`);
    }
    if (rustSource !== undefined) {
      const rustHashes = [...rustSource.matchAll(/0x([0-9a-fA-F_]+)/g)].map((match) =>
        Number.parseInt(match[1].replace(/_/g, ''), 16),
      );

      if (rustHashes.length !== flagHashes.length) {
        fail(
          `treasures: the save parser tracks ${rustHashes.length} chest flag(s) ` +
            `but the flag table has ${flagHashes.length}`,
        );
      }

      const rustSet = new Set(rustHashes);
      const missingFromRust = flagHashes.filter((hash) => !rustSet.has(hash));
      if (missingFromRust.length > 0) {
        fail(
          `treasures: ${missingFromRust.length} chest flag(s) missing from the save parser, ` +
            `first: ${missingFromRust[0]}`,
        );
      }
    }
  }
}

if (failures.length > 0) {
  console.error(`map object dataset validation failed (${failures.length} issue(s)):`);
  for (const failure of failures) console.error(`- ${failure}`);
  process.exit(1);
}

const summary = [
  ...Object.entries(EXPECTED_TOTALS).map(([type, total]) => `${total} ${type}`),
  `${EXPECTED_TREASURES} treasures`,
].join(', ');
console.log(`map object dataset OK: ${summary}`);
