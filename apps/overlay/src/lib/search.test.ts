import { describe, expect, it } from 'vitest'
import type { Category, Pin } from './game'
import { PinSearch } from './search'

const categories: Category[] = [
  { id: 'weapon', name: 'Weapons & Armor', color: '#f00', icon: 'game:KnightVIcon', visible: true },
  { id: 'tool', name: 'Tools & Gear', color: '#0f0', icon: 'game:WrenchIcon', visible: true },
]
const pin = (id: string, name: string, category: string, description?: string): Pin => ({
  id,
  name,
  category,
  description,
  position: { x: 0, y: 0, z: 0 },
})
const pins = [
  pin('1', 'Shotgun', 'weapon'),
  pin('2', 'Rope Gun', 'tool', 'Fires a zipline rope'),
  pin('3', 'Modern Axe', 'tool'),
  pin('4', 'Golden Armor', 'weapon'),
]

describe('PinSearch', () => {
  const s = new PinSearch(pins, categories)

  it('matches by prefix and tolerates typos', () => {
    expect(s.search('shot')).toEqual(['1'])
    expect(s.search('shotgum')).toContain('1')
  })

  it('searches categories and descriptions', () => {
    expect(s.search('weapons').sort()).toEqual(['1', '4'])
    expect(s.search('zipline')).toEqual(['2'])
  })

  it('requires every term to match', () => {
    expect(s.search('modern axe')).toEqual(['3'])
  })

  it('returns nothing for blank queries', () => {
    expect(s.search('   ')).toEqual([])
  })
})
