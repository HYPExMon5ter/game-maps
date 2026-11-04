import { describe, expect, it } from 'vitest'
import { completion, percent } from './completion'
import type { Category, Pin } from './game'

const cat = (id: string, autoFound?: boolean): Category => ({ id, name: id, color: '#fff', icon: 'x', visible: true, autoFound })
const pin = (id: string, category: string, custom = false): Pin => ({ id, name: id, category, position: { x: 0, y: 0, z: 0 }, custom })

describe('completion', () => {
  const categories = [cat('weapons', true), cat('caves'), cat('notes', true), cat('custom')]
  const pins = [pin('w1', 'weapons'), pin('w2', 'weapons'), pin('c1', 'caves'), pin('c2', 'caves'), pin('n1', 'notes'), pin('m1', 'custom', true)]

  it('counts collectible categories, and others once something in them is found', () => {
    const none = completion(pins, categories, [])
    expect(none.categories.map((c) => c.category.id)).toEqual(['weapons', 'notes'])
    expect([none.found, none.total]).toEqual([0, 3])

    const some = completion(pins, categories, ['w1', 'c2', 'm1'])
    expect(some.categories.map((c) => [c.category.id, c.found, c.total])).toEqual([
      ['weapons', 1, 2],
      ['caves', 1, 2],
      ['notes', 0, 1],
    ])
    expect(some.categories[0].left.map((p) => p.id)).toEqual(['w2'])
    expect([some.found, some.total]).toEqual([2, 5])
  })

  it('only shows 100% when everything is found', () => {
    expect(percent(199, 200)).toBe(99)
    expect(percent(200, 200)).toBe(100)
    expect(percent(0, 0)).toBe(0)
  })
})
