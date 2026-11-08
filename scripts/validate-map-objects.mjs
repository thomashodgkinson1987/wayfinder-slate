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
 * - each type carries the fields used to label its markers.
 *
 * Run with `node scripts/validate-map-objects.mjs`.
 */
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const DATASET_PATH = path.join(ROOT, 'frontend/static/data/map_objects.json');

const EXPECTED_TOTALS = {
  shrines: 136,
  towers: 15,
  koroks: 900,
  memories: 14,
  locations: 187,
  enemies: 84,
};

const COORDINATE_LIMIT = 6000;

const EXPECTED_FLAGS = {
  shrines: sequence('Clear_Dungeon', 0, 136, 3),
  towers: sequence('MapTower_', 1, 15, 2),
  memories: sequence('IsGet_MemoryPhoto_', 0, 14, 3),
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

if (dataset.version !== 1) {
  fail(`expected dataset version 1, found ${JSON.stringify(dataset.version)}`);
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

    for (const axis of ['x', 'z']) {
      const value = object[axis];
      if (typeof value !== 'number' || !Number.isFinite(value)) {
        fail(`${where}: expected a finite ${axis} coordinate`);
      } else if (Math.abs(value) > COORDINATE_LIMIT) {
        fail(`${where}: ${axis}=${value} is outside ±${COORDINATE_LIMIT}`);
      }
    }

    checkLabel(type, object, where);
  });

  checkFlagSequence(type, objects);
}

if (failures.length > 0) {
  console.error(`map object dataset validation failed (${failures.length} issue(s)):`);
  for (const failure of failures) console.error(`- ${failure}`);
  process.exit(1);
}

const summary = Object.entries(EXPECTED_TOTALS)
  .map(([type, total]) => `${total} ${type}`)
  .join(', ');
console.log(`map object dataset OK: ${summary}`);
