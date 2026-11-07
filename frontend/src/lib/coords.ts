import L from 'leaflet';

/**
 * The BotW map is 24,000 px wide at maximum zoom, built from 750 px tiles
 * (32 x 32 at zoom 5). Zelda Dungeon's marker coordinates use Leaflet lat/lng
 * directly: lng = world x, lat = -world z.
 *
 * Savage Wind reports x = left/right, y = height, z = forward/back. The map
 * therefore uses (x, z), with z negated so north is up.
 */
export const MAP_SIZE = 24_000;
export const TILE_SIZE = 750;
export const MAX_ZOOM = 5;
export const INITIAL_CENTER: L.LatLngExpression = [-3750, -1900];
export const INITIAL_ZOOM = 3;

export function gameToLatLng(x: number, z: number): L.LatLngTuple {
  return [-z, x];
}

export function createBotwCrs(): L.CRS {
  const scale = TILE_SIZE / MAP_SIZE;
  const offset = TILE_SIZE / 2;
  return L.extend({}, L.CRS.Simple, {
    transformation: new L.Transformation(scale, offset, -scale, offset),
  });
}

export function botwBounds(crs: L.CRS): L.LatLngBounds {
  return L.latLngBounds(
    crs.pointToLatLng(L.point(0, MAP_SIZE), MAX_ZOOM),
    crs.pointToLatLng(L.point(MAP_SIZE, 0), MAX_ZOOM),
  );
}
