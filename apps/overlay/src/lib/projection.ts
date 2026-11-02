// Game world <-> map conversion.
//
// The map image covers the world rectangle [minEast, maxEast] x [minNorth, maxNorth]
// (north up) and is placed exactly on one Web Mercator tile: (tileX, tileY) at zoom
// `tileZoom`. The rest of the Mercator world is empty background. Placing the map on
// a single tile keeps the conversion exact and the tiling standard; using a tile
// below zoom 0 (tileZoom >= 1) leaves room around the map so MapLibre can zoom out
// far enough to show all of it on any screen aspect ratio.

import type { Vec3 } from './protocol'

export type Axis = 'x' | 'y' | 'z'

export interface Projection {
  east: Axis
  north: Axis
  up: Axis
  minEast: number
  maxEast: number
  minNorth: number
  maxNorth: number
  /** Mercator tile the map image occupies (XYZ scheme). Defaults to the whole world. */
  tileZoom?: number
  tileX?: number
  tileY?: number
}

export interface LngLat {
  lng: number
  lat: number
}

const DEG = 180 / Math.PI

/** Normalized Mercator origin (top-left, XYZ) and size of the map tile. */
function placement(p: Projection) {
  const size = 1 / 2 ** (p.tileZoom ?? 0)
  return { u0: (p.tileX ?? 0) * size, v0: (p.tileY ?? 0) * size, size }
}

export function toLngLat(p: Projection, pos: Pick<Vec3, Axis>): LngLat {
  const { u0, v0, size } = placement(p)
  const u = u0 + (size * (pos[p.east] - p.minEast)) / (p.maxEast - p.minEast)
  const v = v0 + (size * (p.maxNorth - pos[p.north])) / (p.maxNorth - p.minNorth)
  return { lng: u * 360 - 180, lat: Math.atan(Math.sinh(Math.PI * (1 - 2 * v))) * DEG }
}

/** Inverse of `toLngLat`. The vertical axis is `up` (unknown from the map alone). */
export function toWorld(p: Projection, ll: LngLat, up = 0): Vec3 {
  const { u0, v0, size } = placement(p)
  const u = (ll.lng + 180) / 360
  const v = (1 - Math.log(Math.tan(Math.PI / 4 + ll.lat / DEG / 2)) / Math.PI) / 2
  const out: Vec3 = { x: 0, y: 0, z: 0 }
  out[p.east] = p.minEast + ((u - u0) / size) * (p.maxEast - p.minEast)
  out[p.north] = p.maxNorth - ((v - v0) / size) * (p.maxNorth - p.minNorth)
  out[p.up] = up
  return out
}

/** Compass bearing (degrees clockwise from north) of a world direction vector. */
export function bearingOf(p: Projection, dir: Pick<Vec3, Axis>): number | null {
  const e = dir[p.east]
  const n = dir[p.north]
  if (Math.abs(e) < 1e-6 && Math.abs(n) < 1e-6) return null
  return (((Math.atan2(e, n) * DEG) % 360) + 360) % 360
}

/** Horizontal distance between two world positions, in world units. */
export function groundDistance(p: Projection, a: Vec3, b: Vec3): number {
  return Math.hypot(a[p.east] - b[p.east], a[p.north] - b[p.north])
}

/** Bounds of the map image as [west, south, east, north] in degrees. */
export function mapBounds(p: Projection): [number, number, number, number] {
  const sw = toLngLat(p, { x: 0, y: 0, z: 0, [p.east]: p.minEast, [p.north]: p.minNorth } as Vec3)
  const ne = toLngLat(p, { x: 0, y: 0, z: 0, [p.east]: p.maxEast, [p.north]: p.maxNorth } as Vec3)
  return [sw.lng, sw.lat, ne.lng, ne.lat]
}

/** Center of the map image. */
export function mapCenter(p: Projection): LngLat {
  return toLngLat(p, {
    x: 0,
    y: 0,
    z: 0,
    [p.east]: (p.minEast + p.maxEast) / 2,
    [p.north]: (p.minNorth + p.maxNorth) / 2,
  } as Vec3)
}

/**
 * MapLibre zoom for a zoom level relative to the map image: at relative 0 the image
 * spans 512 px, and each step doubles it. Styling uses this so it doesn't depend on
 * where the image is placed in the Mercator world.
 */
export function mapZoom(p: Projection, relative: number): number {
  return relative + (p.tileZoom ?? 0)
}

/** Zoom at which the map image spans `pixels` (MapLibre: 512 px world at zoom 0). */
export function zoomToFit(p: Projection, pixels: number): number {
  return Math.log2(pixels / 512) + (p.tileZoom ?? 0)
}
