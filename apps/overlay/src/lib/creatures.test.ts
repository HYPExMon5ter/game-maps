import { describe, expect, it } from 'vitest'
import { creatureName, visibleCreatures } from './creatures'
import type { Creature } from './protocol'

describe('creatureName', () => {
  it('splits the game type names into words', () => {
    expect(creatureName('MuddyFemale')).toBe('Muddy female')
    expect(creatureName('Fingers')).toBe('Fingers')
    expect(creatureName('John2')).toBe('John 2')
    expect(creatureName('Slug_Deprecated')).toBe('Slug deprecated')
  })
})

describe('visibleCreatures', () => {
  const at = { x: 0, y: 0, z: 0 }
  const all: Creature[] = [
    { id: '1', kind: 'hostile', name: 'Fingers', position: at },
    { id: '2', kind: 'animal', name: 'Deer', position: at },
    { id: '3', kind: 'friendly', name: 'Virginia', position: at },
  ]

  it('follows the settings', () => {
    expect(visibleCreatures(all, false, true)).toEqual([])
    // Friendly ones are companions, drawn with the players.
    expect(visibleCreatures(all, true, true).map((c) => c.id)).toEqual(['1', '2'])
    expect(visibleCreatures(all, true, false).map((c) => c.id)).toEqual(['1'])
  })
})
