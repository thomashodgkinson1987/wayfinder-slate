import L from 'leaflet';

/**
 * The BotW tile pyramid is 24,000 px wide at maximum zoom (32 x 32 tiles of
 * 750 px at zoom 5). Marker coordinates in `map_objects.json`, however, are in
 * game metres and span roughly ±6,000 (see scripts/validate-map-objects.mjs).
 *
 * Zelda Dungeon's own CRS divides the 24,000 px map by its ~±12,000 pixel
 * coordinate space. Wayfinder's coordinates are half that scale, so reusing the
 * same denominator would place every marker at half its true distance from the
 * centre, bunching them together. Dividing by the coordinate span instead
 * (2 * 6,000 = 12,000) restores 2 px per game metre at zoom 5.
 *
 * Coordinates use Leaflet lat/lng directly: lng = world x, lat = -world z, so
 * the map uses (x, z) with z negated to keep north up.
 */
export const MAP_SIZE = 24_000;
/** Full game-coordinate span (±6,000) that the 24,000 px map covers. */
export const WORLD_SIZE = 12_000;
export const TILE_SIZE = 750;
export const MAX_ZOOM = 5;
export const INITIAL_CENTER: L.LatLngExpression = [0, 0];
export const INITIAL_ZOOM = 2;

export function gameToLatLng(x: number, z: number): L.LatLngTuple {
  return [-z, x];
}

export function createBotwCrs(): L.CRS {
  const scale = TILE_SIZE / WORLD_SIZE;
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
