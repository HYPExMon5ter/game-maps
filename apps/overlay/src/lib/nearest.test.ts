import { describe, expect, it } from 'vitest'
import type { Category, Pin } from './game'
import { nearestUnfound } from './nearest'
import type { Projection } from './projection'
import type { LocalPlayer } from './protocol'

const projection = { east: 'x', north: 'z' } as Projection
const category = { id: 'food' } as Category
const local: LocalPlayer = { position: { x: 0, y: 0, z: 0 }, forward: null, layer: 'surface' }
const pin = (id: string, x: number, more: Partial<Pin> = {}): Pin => ({ id, category: 'food', name: id, position: { x, y: 0, z: 0 }, ...more })

describe('nearestUnfound', () => {
  it('finds the closest pin', () => {
    expect(nearestUnfound([pin('far', 50), pin('near', 10)], [category], [], local, projection)?.id).toBe('near')
  })

  it('skips found, custom, hidden and other pins', () => {
    const pins = [pin('found', 1), pin('custom', 2, { custom: true }), pin('other', 3, { category: 'water' }), pin('foggy', 4), pin('left', 9)]
    expect(nearestUnfound(pins, [category], ['found'], local, projection, (p) => p.id === 'foggy')?.id).toBe('left')
  })

  it('prefers the layer the player is on', () => {
    const pins = [pin('below', 1, { layer: 'caves' }), pin('above', 40)]
    expect(nearestUnfound(pins, [category], [], local, projection)?.id).toBe('above')
    expect(nearestUnfound(pins, [category], [], { ...local, layer: 'caves' }, projection)?.id).toBe('below')
  })

  it('falls back to other layers, and to nothing', () => {
    expect(nearestUnfound([pin('below', 1, { layer: 'caves' })], [category], [], local, projection)?.id).toBe('below')
    expect(nearestUnfound([], [category], [], local, projection)).toBeNull()
  })
})
