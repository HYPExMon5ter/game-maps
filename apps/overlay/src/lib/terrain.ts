// Terrain heights from a map pack's elevation tiles (`3d/dem`, Terrain-RGB WebP), for
// placing a point picked on the 2D map at ground level and for the 3D view's ground.

import { gameAssetUrl, type TerrainInfo } from './map3d'
import type { Projection } from './projection'
import type { Vec3 } from './protocol'

export interface HeightTile {
  heights: Float32Array
  size: number
}

/** Terrain-RGB pixels (RGBA) to heights: height = -10000 + (R*65536 + G*256 + B) / 10. */
export function decodeTerrainRgb(px: Uint8ClampedArray | Uint8Array, count: number): Float32Array {
  const heights = new Float32Array(count)
  for (let i = 0; i < count; i++) heights[i] = -10000 + (px[i * 4] * 65536 + px[i * 4 + 1] * 256 + px[i * 4 + 2]) / 10
  return heights
}

const tiles = new Map<string, Promise<HeightTile | null>>()

/** One elevation tile, decoded once (row 0 north). Null when the pack has none. */
export function heightTile(gameId: string, terrain: TerrainInfo, z: number, x: number, y: number): Promise<HeightTile | null> {
  const url = gameAssetUrl(gameId, terrain.url).replace('{z}', String(z)).replace('{x}', String(x)).replace('{y}', String(y))
  let tile = tiles.get(url)
  if (!tile) {
    tile = (async () => {
      const r = await fetch(url)
      if (!r.ok) return null
      // Exact values: no color management or premultiplying on the way.
      const bitmap = await createImageBitmap(await r.blob(), { colorSpaceConversion: 'none', premultiplyAlpha: 'none' })
      const size = bitmap.width
      const ctx = new OffscreenCanvas(size, size).getContext('2d', { willReadFrequently: true })!
      ctx.drawImage(bitmap, 0, 0)
      bitmap.close()
      return { heights: decodeTerrainRgb(ctx.getImageData(0, 0, size, size).data, size * size), size }
    })().catch(() => null)
    tiles.set(url, tile)
  }
  return tile
}

/** The coarsest tile, which covers the whole map. */
export function wholeMapTile(gameId: string, terrain: TerrainInfo, p: Projection): Promise<HeightTile | null> {
  return heightTile(gameId, terrain, terrain.minNativeZoom, p.tileX ?? 0, p.tileY ?? 0)
}

/** Where a world position falls in the finest tiles: tile x, y and pixel coordinates. */
export function tilePixel(terrain: TerrainInfo, p: Projection, east: number, north: number) {
  const n = 2 ** (terrain.maxNativeZoom - (p.tileZoom ?? 0))
  const u = Math.min(Math.max((east - p.minEast) / (p.maxEast - p.minEast), 0), 1 - 1e-9) * n
  const v = Math.min(Math.max((p.maxNorth - north) / (p.maxNorth - p.minNorth), 0), 1 - 1e-9) * n
  return {
    x: (p.tileX ?? 0) * n + Math.floor(u),
    y: (p.tileY ?? 0) * n + Math.floor(v),
    px: (u - Math.floor(u)) * terrain.tileSize,
    py: (v - Math.floor(v)) * terrain.tileSize,
  }
}

/** Bilinear sample of a tile at pixel coordinates (pixel centers at .5). */
export function sampleTile(t: HeightTile, px: number, py: number): number {
  const fx = Math.min(Math.max(px - 0.5, 0), t.size - 1)
  const fy = Math.min(Math.max(py - 0.5, 0), t.size - 1)
  const x0 = Math.floor(fx)
  const y0 = Math.floor(fy)
  const x1 = Math.min(x0 + 1, t.size - 1)
  const y1 = Math.min(y0 + 1, t.size - 1)
  const h = (x: number, y: number) => t.heights[y * t.size + x]
  const top = h(x0, y0) + (h(x1, y0) - h(x0, y0)) * (fx - x0)
  const bottom = h(x0, y1) + (h(x1, y1) - h(x0, y1)) * (fx - x0)
  return top + (bottom - top) * (fy - y0)
}

/** The terrain height under a world position, or null without elevation data. */
export async function terrainHeight(gameId: string, terrain: TerrainInfo, p: Projection, at: Vec3): Promise<number | null> {
  const { x, y, px, py } = tilePixel(terrain, p, at[p.east], at[p.north])
  const tile = await heightTile(gameId, terrain, terrain.maxNativeZoom, x, y)
  return tile ? sampleTile(tile, px, py) : null
}
