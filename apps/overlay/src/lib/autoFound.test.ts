import { describe, expect, it } from 'vitest'
import { pinsReached } from './autoFound'
import type { Category, Pin } from './game'
import type { LocalPlayer } from './protocol'
import type { Projection } from './projection'

const projection: Projection = { east: 'x', north: 'z', up: 'y', minEast: -100, maxEast: 100, minNorth: -100, maxNorth: 100 }
const categories: Category[] = [
  { id: 'weapon', name: 'Weapons', color: '#f00', icon: 'builtin:pin', visible: true, autoFound: true },
  { id: 'cave', name: 'Caves', color: '#00f', icon: 'builtin:pin', visible: true },
]
const pin = (id: string, category: string, x: number, y: number, extra: Partial<Pin> = {}): Pin => ({
  id,
  category,
  name: id,
  position: { x, y, z: 0 },
  ...extra,
})
const at = (x: number, y: number, layer: LocalPlayer['layer'] = 'surface'): LocalPlayer => ({ position: { x, y, z: 0 }, forward: null, layer })

describe('pinsReached', () => {
  const pins = [
    pin('near', 'weapon', 1, 0),
    pin('far', 'weapon', 10, 0),
    pin('above', 'weapon', 0, 8),
    pin('cave-entrance', 'cave', 0, 0),
    pin('underground', 'weapon', 0, 0, { layer: 'caves' }),
    pin('mine', 'weapon', 0, 0, { custom: true }),
  ]

  it('marks collectibles the player stands at, on their layer', () => {
    expect(pinsReached(pins, categories, new Set(), at(0, 0), projection, 1).map((p) => p.id)).toEqual(['near'])
    expect(pinsReached(pins, categories, new Set(), at(0, 0, 'caves'), projection, 1).map((p) => p.id)).toEqual(['underground'])
  })

  it('skips pins already found', () => {
    expect(pinsReached(pins, categories, new Set(['near']), at(0, 0), projection, 1)).toEqual([])
  })
})
