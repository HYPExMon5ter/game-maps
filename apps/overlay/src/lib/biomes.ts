// A generated world's biomes, a byte per cell of its map (`biomes.bin`, row 0 in the south),
// so the map can say what's under the cursor and where the nearest of each is.

import { worldMapBase } from './game'
import type { Projection } from './projection'
import type { Vec3 } from './protocol'

export interface BiomeGrid {
  size: number
  cells: Uint8Array
  minEast: number
  minNorth: number
  /** World units per cell. */
  cell: number
}

export async function loadBiomes(gameId: string, worldId: string, p: Projection): Promise<BiomeGrid | null> {
  const res = await fetch(`${worldMapBase(gameId, worldId)}/biomes.bin`)
  if (!res.ok) return null
  const cells = new Uint8Array(await res.arrayBuffer())
  const size = Math.round(Math.sqrt(cells.length))
  if (size === 0 || size * size !== cells.length) return null
  return { size, cells, minEast: p.minEast, minNorth: p.minNorth, cell: (p.maxEast - p.minEast) / size }
}

/** The biome at a world point, or null off the map. */
export function biomeAt(grid: BiomeGrid, east: number, north: number): number | null {
  const col = Math.floor((east - grid.minEast) / grid.cell)
  const row = Math.floor((north - grid.minNorth) / grid.cell)
  if (col < 0 || row < 0 || col >= grid.size || row >= grid.size) return null
  return grid.cells[row * grid.size + col]
}

/**
 * The closest cell of `biome` to (east, north), in rings outward, so it's quick when it's
 * near. `allowed`: leaves out cells the player shouldn't be told about.
 */
export function nearestBiome(grid: BiomeGrid, biome: number, east: number, north: number, allowed: (east: number, north: number) => boolean = () => true): { east: number; north: number } | null {
  const { size, cells, cell } = grid
  const col0 = Math.floor((east - grid.minEast) / cell)
  const row0 = Math.floor((north - grid.minNorth) / cell)
  const center = (i: number, min: number) => min + (i + 0.5) * cell
  for (let ring = 0; ring < size; ring++) {
    let best: { east: number; north: number } | null = null
    let distance = Infinity
    for (let row = row0 - ring; row <= row0 + ring; row++) {
      if (row < 0 || row >= size) continue
      const edge = row === row0 - ring || row === row0 + ring
      for (let col = col0 - ring; col <= col0 + ring; col += edge ? 1 : 2 * ring || 1) {
        if (col < 0 || col >= size || cells[row * size + col] !== biome) continue
        const e = center(col, grid.minEast)
        const n = center(row, grid.minNorth)
        const d = Math.hypot(e - east, n - north)
        if (d < distance && allowed(e, n)) {
          best = { east: e, north: n }
          distance = d
        }
      }
    }
    if (best) return best
  }
  return null
}

/** A world position on the ground at (east, north), height unknown. */
export function groundPoint(p: Projection, east: number, north: number): Vec3 {
  const v: Vec3 = { x: 0, y: 0, z: 0 }
  v[p.east] = east
  v[p.north] = north
  return v
}
