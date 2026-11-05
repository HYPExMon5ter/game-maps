import { describe, expect, it } from 'vitest'
import { DEFAULT_WORLD, DEFAULT_WORLD_INFO, EMPTY_INDEX, EMPTY_WORLD_DATA, hasData, isUnsaved, keys, mergeWorldData, pickerWorlds, recordPlayed, worldLabel } from './worlds'

describe('worlds', () => {
  it('keeps the default world where data lived before worlds', () => {
    expect(keys.data('sotf', DEFAULT_WORLD)).toBe('game:sotf')
    expect(keys.session('sotf', DEFAULT_WORLD)).toBe('session:sotf')
    expect(keys.data('sotf', 'sp:42')).toBe('world:sotf:sp:42')
    expect(keys.session('sotf', 'sp:42')).toBe('session:sotf:sp:42')
  })

  it('records worlds as they are played, most recent first', () => {
    let index = recordPlayed(EMPTY_INDEX, { id: 'sp:1', name: null, kind: 'single_player' }, 100)
    index = recordPlayed(index, { id: 'client:9', name: 'Bob', kind: 'joined' }, 200)
    index = recordPlayed(index, { id: 'sp:1', name: null, kind: 'single_player' }, 300)
    expect(index.worlds.map((w) => [w.id, w.lastPlayed])).toEqual([
      ['sp:1', 300],
      ['client:9', 200],
    ])
    expect(index.last).toBe('sp:1')
  })

  it('offers the default world only while it has data, or when nothing else exists', () => {
    expect(pickerWorlds(EMPTY_INDEX, false)).toEqual([DEFAULT_WORLD_INFO])
    const index = recordPlayed(EMPTY_INDEX, { id: 'sp:1', name: null, kind: 'single_player' }, 1)
    expect(pickerWorlds(index, false).map((w) => w.id)).toEqual(['sp:1'])
    expect(pickerWorlds(index, true).map((w) => w.id)).toEqual(['sp:1', DEFAULT_WORLD])
  })

  it('labels worlds by name, else by kind and save number', () => {
    expect(worldLabel({ id: 'world:abc', name: 'Island', kind: 'hosted', lastPlayed: 1 })).toBe('Island')
    expect(worldLabel({ id: 'sp:1234', name: null, kind: 'single_player', lastPlayed: 1 })).toBe('Single player save 1234')
    expect(worldLabel(DEFAULT_WORLD_INFO)).toBe('Default world')
  })

  it('knows when there is data to move', () => {
    expect(hasData(EMPTY_WORLD_DATA)).toBe(false)
    expect(hasData({ ...EMPTY_WORLD_DATA, found: ['x'] })).toBe(true)
  })

  it('merges pins and found items without duplicates', () => {
    const pin = (id: string) => ({ id, name: id, category: 'custom', position: { x: 0, y: 0, z: 0 } })
    const merged = mergeWorldData({ ...EMPTY_WORLD_DATA, customPins: [pin('a')], found: ['x'] }, { ...EMPTY_WORLD_DATA, customPins: [pin('a'), pin('b')], found: ['x', 'y'] })
    expect(merged.customPins.map((p) => p.id)).toEqual(['a', 'b'])
    expect(merged.found).toEqual(['x', 'y'])
  })

  it('keeps notes and timers when merging, the first world winning', () => {
    const merged = mergeWorldData({ ...EMPTY_WORLD_DATA, notes: { a: 'mine' }, timers: { t: 1 } }, { ...EMPTY_WORLD_DATA, notes: { a: 'old', b: 'theirs' }, timers: { t: 2, u: 3 } })
    expect(merged.notes).toEqual({ a: 'mine', b: 'theirs' })
    expect(merged.timers).toEqual({ t: 1, u: 3 })
  })

  it('tells unsaved new games apart', () => {
    expect(isUnsaved('unsaved:12-1')).toBe(true)
    expect(isUnsaved('sp:12')).toBe(false)
    expect(worldLabel({ id: 'unsaved:12-1', name: null, kind: 'single_player', lastPlayed: 0 })).toBe('New game (not saved yet)')
  })
})
