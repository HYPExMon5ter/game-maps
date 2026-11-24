import { describe, expect, it } from 'vitest'
import { pinsReached, reachInputs, sameInputs } from './autoFound'
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

describe('reachInputs', () => {
  const pins = [pin('edge', 'weapon', 2.6, 0)]
  const found: string[] = []
  const inputs = (local: LocalPlayer, over: { pins?: Pin[]; found?: string[]; world?: string } = {}) =>
    reachInputs(over.pins ?? pins, categories, over.found ?? found, over.world ?? 'sp:1', local, projection, 1)

  it('is the same while nothing a check goes by has changed', () => {
    expect(sameInputs(inputs(at(0, 0)), inputs(at(0, 0)))).toBe(true)
    expect(sameInputs(inputs(at(0, 0)), [])).toBe(false)
  })

  it('changes with the smallest step, so a pin edged into range is found', () => {
    const before = at(0, 0)
    const after = at(0.11, 0)
    expect(pinsReached(pins, categories, new Set(), before, projection, 1)).toEqual([])
    expect(sameInputs(inputs(before), inputs(after))).toBe(false)
    expect(pinsReached(pins, categories, new Set(), after, projection, 1).map((p) => p.id)).toEqual(['edge'])
  })

  it('changes with the layer, the pins, what is found and the world', () => {
    const here = inputs(at(0, 0))
    expect(sameInputs(here, inputs(at(0, 0, 'caves')))).toBe(false)
    expect(sameInputs(here, inputs(at(0, 0), { pins: [...pins] }))).toBe(false)
    expect(sameInputs(here, inputs(at(0, 0), { found: ['edge'] }))).toBe(false)
    expect(sameInputs(here, inputs(at(0, 0), { world: 'sp:2' }))).toBe(false)
  })
})
