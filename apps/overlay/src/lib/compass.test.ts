import { describe, expect, it } from 'vitest'
import { compassReading, formatDistance } from './compass'
import type { Projection } from './projection'

const p: Projection = { east: 'x', north: 'z', up: 'y', minEast: -2000, maxEast: 2000, minNorth: -2000, maxNorth: 2000 }

describe('compassReading', () => {
  it('is straight ahead when facing the target', () => {
    const r = compassReading(p, { position: { x: 0, y: 10, z: 0 }, forward: { x: 0, y: 0, z: 1 }, layer: 'surface' }, { x: 0, y: 40, z: 100 }, 1)
    expect(r.relative).toBeCloseTo(0)
    expect(r.distance).toBeCloseTo(100)
    expect(r.rise).toBeCloseTo(30)
  })

  it('points right/left/behind relative to the camera', () => {
    const facingNorth = { position: { x: 0, y: 0, z: 0 }, forward: { x: 0, y: -0.3, z: 1 }, layer: 'surface' as const }
    expect(compassReading(p, facingNorth, { x: 50, y: 0, z: 0 }, 1).relative).toBeCloseTo(90)
    expect(compassReading(p, facingNorth, { x: -50, y: 0, z: 0 }, 1).relative).toBeCloseTo(-90)
    expect(Math.abs(compassReading(p, facingNorth, { x: 0, y: 0, z: -50 }, 1).relative!)).toBeCloseTo(180)
  })

  it('has no relative bearing without a camera direction', () => {
    const r = compassReading(p, { position: { x: 0, y: 0, z: 0 }, forward: null, layer: 'surface' }, { x: 3, y: 0, z: 4 }, 1)
    expect(r.relative).toBeNull()
    expect(r.distance).toBeCloseTo(5)
  })

  it('formats distances', () => {
    expect(formatDistance(42.4)).toBe('42 m')
    expect(formatDistance(1534)).toBe('1.53 km')
  })
})
