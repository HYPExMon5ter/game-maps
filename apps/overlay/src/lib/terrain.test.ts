import { describe, expect, it } from 'vitest'
import { decodeTerrainRgb, sampleTile, tilePixel } from './terrain'
import type { TerrainInfo } from './map3d'
import type { Projection } from './projection'

const SOTF: Projection = { east: 'x', north: 'z', up: 'y', minEast: -2000, maxEast: 2000, minNorth: -2000, maxNorth: 2000, tileZoom: 3, tileX: 3, tileY: 3 }
const TERRAIN: TerrainInfo = { url: '3d/dem/{z}/{x}/{y}.webp', tileSize: 256, minNativeZoom: 3, maxNativeZoom: 5, minHeight: 0, maxHeight: 0 }

describe('decodeTerrainRgb', () => {
  it('reads Terrain-RGB heights', () => {
    const v = (h: number) => Math.round((h + 10000) * 10)
    const px = new Uint8Array([...[123.4, -19].flatMap((h) => [v(h) >> 16, (v(h) >> 8) & 255, v(h) & 255, 255])])
    const [a, b] = decodeTerrainRgb(px, 2)
    expect(a).toBeCloseTo(123.4, 1)
    expect(b).toBeCloseTo(-19, 1)
  })
})

describe('tilePixel', () => {
  it('finds the finest tile and pixel under a world position', () => {
    // Zoom 5 splits the map tile into 4x4; the north-west corner is tile (12, 12).
    expect(tilePixel(TERRAIN, SOTF, -2000, 2000)).toEqual({ x: 12, y: 12, px: 0, py: 0 })
    const center = tilePixel(TERRAIN, SOTF, 0, 0)
    expect(center).toEqual({ x: 14, y: 14, px: 0, py: 0 })
    expect(tilePixel(TERRAIN, SOTF, 2000, -2000)).toMatchObject({ x: 15, y: 15 })
  })
})

describe('sampleTile', () => {
  it('interpolates between pixel centers', () => {
    const t = { size: 2, heights: new Float32Array([0, 10, 20, 30]) }
    expect(sampleTile(t, 0.5, 0.5)).toBe(0)
    expect(sampleTile(t, 1, 0.5)).toBe(5)
    expect(sampleTile(t, 1, 1)).toBe(15)
    expect(sampleTile(t, 9, 9)).toBe(30)
  })
})
