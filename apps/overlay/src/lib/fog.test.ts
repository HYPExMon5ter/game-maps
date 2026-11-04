import { describe, expect, it } from 'vitest'
import { emptyFog, exploredAt, exploredFraction, fogPixels, isExplored, loadFog, reveal, saveFog } from './fog'
import type { Projection } from './projection'

const p: Projection = { east: 'x', north: 'z', up: 'y', minEast: -400, maxEast: 400, minNorth: -400, maxNorth: 400 }

describe('fog of war', () => {
  it('uses 8 m cells over the map rectangle', () => {
    const fog = emptyFog(p, 1)
    expect([fog.cell, fog.width, fog.height]).toEqual([8, 100, 100])
    expect(exploredFraction(fog)).toBe(0)
  })

  it('caps the bitmap size for huge maps', () => {
    const fog = emptyFog({ ...p, minEast: -50_000, maxEast: 50_000 }, 1)
    expect(fog.width).toBeLessThanOrEqual(2048)
  })

  it('reveals a circle around a point, once', () => {
    const fog = emptyFog(p, 1)
    expect(reveal(fog, 0, 0, 40)).toBe(true)
    expect(isExplored(fog, 50, 50)).toBe(true)
    expect(isExplored(fog, 50 + 6, 50)).toBe(false)
    expect(isExplored(fog, 0, 0)).toBe(false)
    expect(reveal(fog, 0, 0, 40)).toBe(false)
    const area = exploredFraction(fog) * 100 * 100
    expect(area).toBeGreaterThan(70)
    expect(area).toBeLessThan(90)
  })

  it('round-trips through its saved form, and starts over if the map changed', () => {
    const fog = emptyFog(p, 1)
    reveal(fog, 100, -200, 30)
    const back = loadFog(saveFog(fog), emptyFog(p, 1))
    expect(back.bits).toEqual(fog.bits)
    const other = emptyFog({ ...p, maxEast: 800 }, 1)
    expect(exploredFraction(loadFog(saveFog(fog), other))).toBe(0)
    expect(exploredFraction(loadFog(null, other))).toBe(0)
  })

  it('draws north up with explored cells clear', () => {
    const fog = emptyFog(p, 1)
    reveal(fog, -396, 396, 1) // the north-west corner cell
    const px = fogPixels(fog, [10, 20, 30])
    expect(Array.from(px.subarray(0, 4))).toEqual([10, 20, 30, 0])
    expect(px[(99 * 100) * 4 + 3]).toBe(255)
  })

  it('looks up whether a world point is explored', () => {
    const fog = emptyFog(p, 1)
    reveal(fog, 100, 100, 20)
    expect(exploredAt(fog, 105, 95)).toBe(true)
    expect(exploredAt(fog, 200, 100)).toBe(false)
    expect(exploredAt(fog, 5000, 0)).toBe(false)
  })
})
