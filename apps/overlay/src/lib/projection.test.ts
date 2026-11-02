import { describe, expect, it } from 'vitest'
import { bearingOf, groundDistance, mapBounds, mapCenter, toLngLat, toWorld, zoomToFit, type Projection } from './projection'

const sotf: Projection = {
  east: 'x',
  north: 'z',
  up: 'y',
  minEast: -2000,
  maxEast: 2000,
  minNorth: -2000,
  maxNorth: 2000,
}

describe('projection', () => {
  it('maps the world center to 0,0 and corners to the mercator edges', () => {
    const c = toLngLat(sotf, { x: 0, y: 0, z: 0 })
    expect(c.lng).toBeCloseTo(0)
    expect(c.lat).toBeCloseTo(0)
    const ne = toLngLat(sotf, { x: 2000, y: 0, z: 2000 })
    expect(ne.lng).toBeCloseTo(180)
    expect(ne.lat).toBeCloseTo(85.0511, 3)
  })

  it('puts north (+z) up and east (+x) right', () => {
    const p = toLngLat(sotf, { x: 500, y: 0, z: 1000 })
    expect(p.lng).toBeGreaterThan(0)
    expect(p.lat).toBeGreaterThan(0)
  })

  it('round-trips world positions', () => {
    for (const [x, z] of [
      [1813.81, 505.53],
      [-1268.91, -1683.49],
      [0, 0],
      [-1999, 1999],
    ]) {
      const w = toWorld(sotf, toLngLat(sotf, { x, y: 0, z }), 42)
      expect(w.x).toBeCloseTo(x, 6)
      expect(w.z).toBeCloseTo(z, 6)
      expect(w.y).toBe(42)
    }
  })

  it('places the map on a single sub-world tile when configured', () => {
    const tiled: Projection = { ...sotf, tileZoom: 2, tileX: 1, tileY: 1 }
    // Tile (1,1) at z2 spans lng [-90, 0] and lat [0, 66.51].
    const nw = toLngLat(tiled, { x: -2000, y: 0, z: 2000 })
    expect(nw.lng).toBeCloseTo(-90)
    expect(nw.lat).toBeCloseTo(66.5133, 3)
    const se = toLngLat(tiled, { x: 2000, y: 0, z: -2000 })
    expect(se.lng).toBeCloseTo(0)
    expect(se.lat).toBeCloseTo(0)
    for (const [x, z] of [
      [1813.81, 505.53],
      [-1268.91, -1683.49],
    ]) {
      const w = toWorld(tiled, toLngLat(tiled, { x, y: 0, z }))
      expect(w.x).toBeCloseTo(x, 6)
      expect(w.z).toBeCloseTo(z, 6)
    }
    expect(zoomToFit(tiled, 512)).toBeCloseTo(2)
    const c = mapCenter(tiled)
    expect(toWorld(tiled, c).x).toBeCloseTo(0)
    expect(mapBounds(tiled)[0]).toBeCloseTo(-90)
  })

  it('computes compass bearings and ground distance', () => {
    expect(bearingOf(sotf, { x: 0, y: 0, z: 1 })).toBeCloseTo(0)
    expect(bearingOf(sotf, { x: 1, y: 0, z: 0 })).toBeCloseTo(90)
    expect(bearingOf(sotf, { x: 0, y: 0, z: -1 })).toBeCloseTo(180)
    expect(bearingOf(sotf, { x: -1, y: 5, z: 0 })).toBeCloseTo(270)
    expect(bearingOf(sotf, { x: 0, y: 1, z: 0 })).toBeNull()
    expect(groundDistance(sotf, { x: 0, y: 100, z: 0 }, { x: 3, y: 0, z: 4 })).toBeCloseTo(5)
  })
})
