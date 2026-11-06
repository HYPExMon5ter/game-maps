import { describe, expect, it } from 'vitest'
import type { Category, Pin } from './game'
import { enteredDungeon, hostPins, regrowthTimers, sharedLocations } from './worldExtras'

const pin = (id: string, x: number, z: number, extra: Partial<Pin> = {}): Pin => ({ id, category: 'dungeon', name: id, position: { x, y: 40, z }, ...extra })
const categories = [
  { id: 'dungeon', name: 'Dungeons', color: '#000', icon: 'builtin:cave', visible: true },
  { id: 'copper', name: 'Copper', color: '#000', icon: 'builtin:gem', visible: true, remembered: true },
] satisfies Category[]

describe('enteredDungeon', () => {
  const dungeons = [pin('near', 0, 0), pin('other', 100, 0)]

  it('goes by where the player stood outside', () => {
    expect(enteredDungeon(dungeons, { x: 95, y: 40, z: 5 }, { x: 10, y: 5040, z: 0 })?.id).toBe('other')
  })

  it('falls back on where they are inside after a teleport', () => {
    expect(enteredDungeon(dungeons, { x: 5000, y: 40, z: 0 }, { x: 10, y: 5040, z: 0 })?.id).toBe('near')
    expect(enteredDungeon(dungeons, null, { x: 90, y: 5040, z: 0 })?.id).toBe('other')
  })

  it('finds nothing with no dungeon around', () => {
    expect(enteredDungeon(dungeons, null, { x: 900, y: 5040, z: 0 })).toBeNull()
  })
})

describe('regrowthTimers', () => {
  it('starts timers on what is growing back', () => {
    expect(regrowthTimers([pin('a', 0, 0, { respawnAt: 500 }), pin('b', 0, 0)], { c: 9 }, 100)).toEqual({ c: 9, a: 500 })
  })

  it('leaves running timers and what is already back', () => {
    expect(regrowthTimers([pin('a', 0, 0, { respawnAt: 500 })], { a: 300 }, 100)).toBeNull()
    expect(regrowthTimers([pin('a', 0, 0, { respawnAt: 50 })], {}, 100)).toBeNull()
  })
})

describe('sharing locations', () => {
  it("sends the world's locations, not what's been seen", () => {
    const sent = sharedLocations([pin('crypt', 1, 2), pin('ore', 3, 4, { category: 'copper' }), pin('mine', 5, 6, { custom: true })], categories)
    expect(sent).toEqual([['crypt', 'dungeon', 'crypt', 1, 40, 2]])
  })

  it('only takes categories this game has', () => {
    const pins = hostPins([['a', 'dungeon', 'A', 1, 2, 3], ['b', 'evil', 'B', 1, 2, 3], ['c', 'copper', 'C', 1, 2, 3]], categories)
    expect(pins).toEqual([{ id: 'a', category: 'dungeon', name: 'A', position: { x: 1, y: 2, z: 3 } }])
  })
})
