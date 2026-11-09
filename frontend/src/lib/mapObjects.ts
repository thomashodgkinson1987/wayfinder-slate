import type { CompletedObjects, MapObject, MapObjectType } from './api';

/** How completed objects should be shown on the map. */
export type CompletionFilter = 'all' | 'unfinished' | 'completed';

/** A point in the save file's game coordinate space (x/z metres). */
export interface Point {
  x: number;
  z: number;
}

type FlagListKey =
  | 'shrines'
  | 'towers'
  | 'memories'
  | 'koroks'
  | 'locations'
  | 'enemies'
  | 'divine_beasts';

/** Which completed-identifier list a named-flag object type uses. */
const COMPLETED_KEY: Record<Exclude<MapObjectType, 'treasure'>, FlagListKey> = {
  shrine: 'shrines',
  tower: 'towers',
  memory: 'memories',
  korok: 'koroks',
  location: 'locations',
  enemy: 'enemies',
  divineBeast: 'divine_beasts',
};

/**
 * Is this map object completed in the given save slot?
 *
 * Named objects are matched by their dataset `flag`; treasure chests are
 * matched by their CRC32 save-flag `hash`. Unknown or incomplete data is
 * treated as not completed.
 */
export function isComplete(
  type: MapObjectType,
  object: MapObject,
  completed: CompletedObjects | null,
): boolean {
  if (completed === null) return false;

  if (type === 'treasure') {
    return typeof object.hash === 'number' && completed.treasures.includes(object.hash);
  }

  const flag = object.flag;
  if (flag === undefined) return false;
  return completed[COMPLETED_KEY[type]].includes(flag);
}

/**
 * Filter objects by completion state. `all` and a missing completion response
 * both return every object unchanged.
 */
export function filterObjects(
  type: MapObjectType,
  objects: MapObject[],
  completed: CompletedObjects | null,
  filter: CompletionFilter,
): MapObject[] {
  if (filter === 'all' || completed === null) return objects;
  const wantComplete = filter === 'completed';
  return objects.filter((object) => isComplete(type, object, completed) === wantComplete);
}

/**
 * Pick the unfinished object nearest to `origin`, measured in game metres.
 * Returns `null` when every object is complete (or the list is empty).
 */
export function nearestUnfinished(
  type: MapObjectType,
  objects: MapObject[],
  completed: CompletedObjects | null,
  origin: Point,
): MapObject | null {
  let nearest: MapObject | null = null;
  let nearestDistance = Number.POSITIVE_INFINITY;

  for (const object of objects) {
    if (isComplete(type, object, completed)) continue;
    const dx = object.x - origin.x;
    const dz = object.z - origin.z;
    const distance = dx * dx + dz * dz;
    if (distance < nearestDistance) {
      nearestDistance = distance;
      nearest = object;
    }
  }

  return nearest;
}