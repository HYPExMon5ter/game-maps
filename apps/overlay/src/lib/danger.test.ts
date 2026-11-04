import { describe, expect, it } from 'vitest'
import { nearbyHostiles } from './danger'
import type { Creature, WorldSnapshot } from './protocol'

const creature = (id: string, kind: Creature['kind'], x: number): Creature => ({ id, kind, name: id, position: { x, y: 0, z: 0 } })

const snap = (creatures: Creature[]): WorldSnapshot => ({
  frame: 0,
  local: { position: { x: 0, y: 0, z: 0 }, forward: null, layer: 'surface' },
  others: [],
  markers: [],
  creatures,
})

describe('nearbyHostiles', () => {
  it('keeps hostiles within the radius, nearest first', () => {
    const threats = nearbyHostiles(snap([creature('far', 'hostile', 50), creature('deer', 'animal', 2), creature('b', 'hostile', 20), creature('a', 'hostile', 5)]), 30)
    expect(threats.map((t) => [t.creature.id, t.distance])).toEqual([
      ['a', 5],
      ['b', 20],
    ])
  })

  it('caps how many it returns and handles no data', () => {
    const many = Array.from({ length: 9 }, (_, i) => creature(`c${i}`, 'hostile', i))
    expect(nearbyHostiles(snap(many), 100, 3)).toHaveLength(3)
    expect(nearbyHostiles(null, 30)).toEqual([])
    expect(nearbyHostiles({ ...snap([]), creatures: undefined }, 30)).toEqual([])
  })
})
