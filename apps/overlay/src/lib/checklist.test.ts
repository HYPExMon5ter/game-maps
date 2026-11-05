import { describe, expect, it } from 'vitest'
import { checklistRows } from './checklist'
import type { Pin } from './game'

const pin = (id: string, name: string, more: Partial<Pin> = {}): Pin => ({ id, name, category: 'key', position: { x: 0, y: 0, z: 0 }, ...more })

describe('checklistRows', () => {
  const pins = [pin('a', 'Guest Keycard'), pin('b', 'Guest Keycard'), pin('c', 'Shovel'), pin('d', 'Shovel', { custom: true })]

  it('links entries to their pins', () => {
    const [row] = checklistRows([{ name: 'Shovel', pins: ['Shovel'] }], pins, [])
    expect(row.pins.map((p) => p.id)).toEqual(['c'])
    expect(row.done).toBe(false)
  })

  it('ticks an entry when any of its pins is found', () => {
    expect(checklistRows([{ name: 'Guest keycard', pins: ['Guest Keycard'] }], pins, ['b'])[0].done).toBe(true)
  })

  it('keeps entries the map has no pins for', () => {
    expect(checklistRows([{ name: 'Golden armor', pins: ['Golden Armor'] }], pins, [])).toEqual([{ name: 'Golden armor', done: false, pins: [] }])
  })
})
