import { describe, expect, it } from 'vitest';
import type { CompletedObjects, MapObject } from './api';
import { filterObjects, isComplete, nearestUnfinished } from './mapObjects';

function completed(overrides: Partial<CompletedObjects> = {}): CompletedObjects {
  return {
    shrines: [],
    towers: [],
    memories: [],
    koroks: [],
    locations: [],
    enemies: [],
    divine_beasts: [],
    treasures: [],
    ...overrides,
  };
}

const KOROK_FLAG = 'MainField_Npc_HiddenKorokGround_3148789636';
const CHEST_HASH = 2_304_095;

const shrine: MapObject = { flag: 'Clear_Dungeon015', name: 'Toto Sah Shrine', x: 0, z: 0 };
const korok: MapObject = { flag: KOROK_FLAG, x: 10, z: 0 };
const chest: MapObject = { id: 'Treasure0001', hash: CHEST_HASH, x: 20, z: 0 };

describe('isComplete', () => {
  it('matches a named save flag', () => {
    expect(isComplete('shrine', shrine, completed())).toBe(false);
    expect(isComplete('shrine', shrine, completed({ shrines: [shrine.flag!] }))).toBe(true);
  });

  it('matches a Korok flag', () => {
    expect(isComplete('korok', korok, completed())).toBe(false);
    expect(isComplete('korok', korok, completed({ koroks: [KOROK_FLAG] }))).toBe(true);
  });

  it('matches a chest by its CRC32 hash', () => {
    expect(isComplete('treasure', chest, completed())).toBe(false);
    expect(isComplete('treasure', chest, completed({ treasures: [CHEST_HASH] }))).toBe(true);
  });

  it('treats missing data as unfinished', () => {
    expect(isComplete('shrine', shrine, null)).toBe(false);
    expect(isComplete('treasure', { ...chest, hash: undefined }, completed({ treasures: [CHEST_HASH] }))).toBe(
      false,
    );
    expect(isComplete('shrine', { x: 0, z: 0 }, completed({ shrines: [shrine.flag!] }))).toBe(false);
  });
});

describe('filterObjects', () => {
  const shrineDone: MapObject = { flag: 'Clear_Dungeon015', x: 0, z: 0 };
  const shrineTodo: MapObject = { flag: 'Clear_Dungeon016', x: 1, z: 0 };
  const chestDone: MapObject = { id: 'T1', hash: CHEST_HASH, x: 2, z: 0 };
  const chestTodo: MapObject = { id: 'T2', hash: 123, x: 3, z: 0 };
  const completion = completed({ shrines: [shrineDone.flag!], treasures: [CHEST_HASH] });

  it('returns every object for the "all" filter', () => {
    expect(filterObjects('shrine', [shrineDone, shrineTodo], completion, 'all')).toEqual([
      shrineDone,
      shrineTodo,
    ]);
  });

  it('hides completed objects for the "unfinished" filter', () => {
    expect(filterObjects('shrine', [shrineDone, shrineTodo], completion, 'unfinished')).toEqual([
      shrineTodo,
    ]);
    expect(filterObjects('treasure', [chestDone, chestTodo], completion, 'unfinished')).toEqual([
      chestTodo,
    ]);
  });

  it('keeps only completed objects for the "completed" filter', () => {
    expect(filterObjects('shrine', [shrineDone, shrineTodo], completion, 'completed')).toEqual([
      shrineDone,
    ]);
    expect(filterObjects('treasure', [chestDone, chestTodo], completion, 'completed')).toEqual([
      chestDone,
    ]);
  });

  it('shows everything when the completion response is missing', () => {
    expect(filterObjects('shrine', [shrineDone, shrineTodo], null, 'unfinished')).toEqual([
      shrineDone,
      shrineTodo,
    ]);
    expect(filterObjects('shrine', [shrineDone, shrineTodo], null, 'completed')).toEqual([
      shrineDone,
      shrineTodo,
    ]);
  });
});

describe('nearestUnfinished', () => {
  const a: MapObject = { flag: 'A', x: 0, z: 0 };
  const b: MapObject = { flag: 'B', x: 30, z: 40 };
  const c: MapObject = { flag: 'C', x: 10, z: 0 };
  const origin = { x: 0, z: 0 };

  it('picks the closest unfinished object', () => {
    expect(nearestUnfinished('shrine', [a, b, c], completed(), origin)).toBe(a);
  });

  it('skips objects that are already complete', () => {
    expect(nearestUnfinished('shrine', [a, b, c], completed({ shrines: ['A'] }), origin)).toBe(c);
    expect(nearestUnfinished('shrine', [a, b, c], completed({ shrines: ['A', 'C'] }), origin)).toBe(b);
  });

  it('falls back to null when nothing is unfinished', () => {
    expect(nearestUnfinished('shrine', [], completed(), origin)).toBeNull();
    expect(
      nearestUnfinished('shrine', [a, b, c], completed({ shrines: ['A', 'B', 'C'] }), origin),
    ).toBeNull();
  });
});