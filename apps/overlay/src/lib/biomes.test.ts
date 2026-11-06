import { describe, expect, it } from 'vitest'
import { biomeAt, nearestBiome, type BiomeGrid } from './biomes'

/** 10 by 10 cells of 10 units around (0, 0), all 0 but the cells listed. */
function grid(cells: [number, number, number][]): BiomeGrid {
  const data = new Uint8Array(100)
  for (const [col, row, biome] of cells) data[row * 10 + col] = biome
  return { size: 10, cells: data, minEast: -50, minNorth: -50, cell: 10 }
}

describe('biomeAt', () => {
  it('reads the cell at a world point, south first', () => {
    expect(biomeAt(grid([[0, 0, 3]]), -45, -45)).toBe(3)
    expect(biomeAt(grid([[0, 0, 3]]), -45, 45)).toBe(0)
  })

  it('has nothing off the map', () => {
    expect(biomeAt(grid([]), 500, 0)).toBeNull()
  })
})

describe('nearestBiome', () => {
  const g = grid([[9, 5, 2], [6, 5, 2]])

  it('finds the closest cell', () => {
    expect(nearestBiome(g, 2, 5, 5)).toEqual({ east: 15, north: 5 })
  })

  it('finds the player standing in it', () => {
    expect(nearestBiome(g, 2, 15, 5)).toEqual({ east: 15, north: 5 })
  })

  it('skips what the player should not be told about', () => {
    expect(nearestBiome(g, 2, 5, 5, (e) => e > 20)).toEqual({ east: 45, north: 5 })
  })

  it('finds nothing when there is none', () => {
    expect(nearestBiome(g, 7, 5, 5)).toBeNull()
  })
})
