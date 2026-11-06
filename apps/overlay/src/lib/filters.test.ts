import { describe, expect, it } from 'vitest'
import { filterGroups, isItemVisible, isPinVisible, onlyFilters, withCategories, withItem, type Filters } from './filters'
import type { Category, GameDefinition, Pin } from './game'

const ore: Category = { id: 'ore', name: 'Ore', color: '#000', icon: 'builtin:gem', visible: false, split: true, mergeNames: { 'Iron vein': 'Iron' } }
const fish: Category = { id: 'fish', name: 'Fish', color: '#000', icon: 'builtin:gem', visible: true }
const custom: Category = { id: 'custom', name: 'My Pins', color: '#fff', icon: 'builtin:pin', visible: true }
const game = { categories: [ore, fish, custom], groups: [{ id: 'nature', name: 'Nature', categories: ['ore', 'fish'] }] } as GameDefinition
const pin = (id: string, category: string, name: string): Pin => ({ id, category, name, position: { x: 0, y: 0, z: 0 } })
const pins = [pin('1', 'ore', 'Iron'), pin('2', 'ore', 'Iron vein'), pin('3', 'ore', 'Copper'), pin('4', 'fish', 'Shark')]
const none: Filters = { hiddenCategories: [], shownCategories: [] }

describe('filterGroups', () => {
  const [nature, own] = filterGroups(game, pins, new Set(['1']))

  it('splits a category into kinds, most common first, and adds up what they hold', () => {
    expect(nature.items.map((i) => [i.label, i.total, i.found])).toEqual([['Iron', 2, 1], ['Copper', 1, 0], ['Fish', 1, 0]])
    expect([nature.total, nature.found]).toEqual([4, 1])
  })

  it('puts what no group holds in My Pins', () => {
    expect(own.name).toBe('My Pins')
    expect(own.items.map((i) => i.key)).toEqual(['custom'])
  })

  it('keeps a split category with nothing in it yet as one line', () => {
    expect(filterGroups(game, [], new Set())[0].items.map((i) => i.key)).toEqual(['ore', 'fish'])
  })
})

describe('visibility', () => {
  it('has kinds follow their category until they are set', () => {
    expect(isPinVisible(ore, pins[0], none)).toBe(false)
    const shown = withCategories(none, ['ore'], true)
    expect(isPinVisible(ore, pins[0], shown)).toBe(true)
    const noIron = withItem(shown, 'ore/Iron', false)
    expect(isPinVisible(ore, pins[0], noIron)).toBe(false)
    expect(isPinVisible(ore, pins[1], noIron)).toBe(false)
    expect(isPinVisible(ore, pins[2], noIron)).toBe(true)
  })

  it('shows one kind while its category is off', () => {
    expect(isPinVisible(ore, pins[0], withItem(none, 'ore/Iron', true))).toBe(true)
    expect(isPinVisible(ore, pins[2], withItem(none, 'ore/Iron', true))).toBe(false)
  })

  it('clears kinds when the whole category is set', () => {
    const filters = withCategories(withItem(none, 'ore/Iron', false), ['ore'], true)
    expect(filters).toEqual({ hiddenCategories: [], shownCategories: ['ore'] })
  })

  it('shows only one thing, a kind or a whole category', () => {
    const iron = onlyFilters(game.categories, 'ore/Iron')
    expect(pins.filter((p) => isPinVisible(game.categories.find((c) => c.id === p.category), p, iron)).map((p) => p.id)).toEqual(['1', '2'])
    const onlyFish = onlyFilters(game.categories, 'fish')
    expect(pins.filter((p) => isPinVisible(game.categories.find((c) => c.id === p.category), p, onlyFish)).map((p) => p.id)).toEqual(['4'])
    const [nature] = filterGroups(game, pins, new Set())
    expect(nature.items.map((i) => isItemVisible(i, iron))).toEqual([true, false, false])
  })
})
