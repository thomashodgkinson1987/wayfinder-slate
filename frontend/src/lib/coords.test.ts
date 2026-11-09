import { describe, expect, it } from 'vitest';
import L from 'leaflet';
import { MAX_ZOOM, botwBounds, createBotwCrs, gameToLatLng } from './coords';

const crs = createBotwCrs();

/** Project a game (x, z) coordinate to a zoom-5 pixel. */
function pixel(x: number, z: number): L.Point {
  return crs.latLngToPoint(L.latLng(gameToLatLng(x, z)), MAX_ZOOM);
}

/**
 * Independent upstream Zelda Dungeon marker coordinates, converted with the
 * documented transform (`x = mapY / 2`, `z = -mapX / 2`). These are not read
 * from the dataset, so they catch a regression in the dataset generator too.
 *
 * Tolerances are ±2 game units and ±4 px, which absorb the marker positional
 * noise without hiding a wrong transform.
 */
const LANDMARKS = [
  {
    name: 'Hyrule Castle (pins.json)',
    mapX: 1223,
    mapY: -508,
    x: -254,
    z: -611.5,
    px: 11492,
    py: 10777,
  },
  {
    name: 'Akkala Tower (pins.json)',
    mapX: 3000,
    mapY: 6616,
    x: 3308,
    z: -1500,
    px: 18616,
    py: 9000,
  },
  {
    name: 'Gerudo Tower (pins.json)',
    mapX: -3657,
    mapY: -7332,
    x: -3666,
    z: 1828.5,
    px: 4668,
    py: 15657,
  },
  {
    name: 'Vah Ruta (wiki.json)',
    mapX: 1101,
    mapY: 6675,
    x: 3337.5,
    z: -550.5,
    px: 18675,
    py: 10899,
  },
  {
    name: 'Vah Naboris (wiki.json)',
    mapX: -5930,
    mapY: -7769,
    x: -3884.5,
    z: 2965,
    px: 4231,
    py: 17930,
  },
];

describe('gameToLatLng', () => {
  it('maps x to lng and negates z so north stays up', () => {
    expect(gameToLatLng(12.5, -3.25)).toEqual([3.25, 12.5]);
    expect(gameToLatLng(-6000, 6000)).toEqual([-6000, -6000]);
  });
});

describe('BotW CRS projection at max zoom', () => {
  it('uses 2 px per game metre offset by 12,000', () => {
    for (const { x, z } of LANDMARKS) {
      const point = pixel(x, z);
      expect(point.x).toBeCloseTo(2 * x + 12000, 4);
      expect(point.y).toBeCloseTo(2 * z + 12000, 4);
    }
  });

  it.each(LANDMARKS)(
    'projects $name to its known pixel',
    ({ mapX, mapY, x, z, px, py }) => {
      // The transform itself: dataset x/z derived from upstream map x/y.
      expect(Math.abs(mapY / 2 - x)).toBeLessThanOrEqual(2);
      expect(Math.abs(-mapX / 2 - z)).toBeLessThanOrEqual(2);

      const point = pixel(x, z);
      expect(Math.abs(point.x - px)).toBeLessThanOrEqual(4);
      expect(Math.abs(point.y - py)).toBeLessThanOrEqual(4);
    },
  );

  it('maps the world corners to the tile pyramid corners', () => {
    const southWest = pixel(-6000, -6000);
    expect(southWest.x).toBeCloseTo(0, 6);
    expect(southWest.y).toBeCloseTo(0, 6);

    const northEast = pixel(6000, 6000);
    expect(northEast.x).toBeCloseTo(24000, 6);
    expect(northEast.y).toBeCloseTo(24000, 6);
  });

  it('round-trips every corner through pointToLatLng and latLngToPoint', () => {
    const corners: [number, number][] = [
      [0, 0],
      [24000, 0],
      [0, 24000],
      [24000, 24000],
    ];
    for (const [x, y] of corners) {
      const roundTripped = crs.latLngToPoint(
        crs.pointToLatLng(L.point(x, y), MAX_ZOOM),
        MAX_ZOOM,
      );
      expect(roundTripped.x).toBeCloseTo(x, 6);
      expect(roundTripped.y).toBeCloseTo(y, 6);
    }
  });

  it('spans the full tile pyramid in botwBounds', () => {
    const bounds = botwBounds(crs);
    const northWest = crs.latLngToPoint(bounds.getNorthWest(), MAX_ZOOM);
    const southEast = crs.latLngToPoint(bounds.getSouthEast(), MAX_ZOOM);

    expect(northWest.x).toBeCloseTo(0, 6);
    expect(northWest.y).toBeCloseTo(0, 6);
    expect(southEast.x).toBeCloseTo(24000, 6);
    expect(southEast.y).toBeCloseTo(24000, 6);
  });
});