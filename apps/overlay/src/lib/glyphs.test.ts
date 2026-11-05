import { describe, expect, it } from 'vitest'
import sonsOfTheForest from '../../../../games/sons-of-the-forest/game.json'
import sunkenland from '../../../../games/sunkenland/game.json'
import { withIconAliases, type Category, type GameDefinition, type Pin } from './game'
import { GLYPH_NAMES, glyphUrl } from './glyphs'

const GAMES = [sonsOfTheForest, sunkenland] as unknown as GameDefinition[]
const builtin = (icon: string) => (icon.startsWith('builtin:') ? icon.slice('builtin:'.length) : null)

describe('glyphs', () => {
  it('draws every symbol', () => {
    for (const name of GLYPH_NAMES) expect(glyphUrl(name)).toContain('svg')
  })

  it('knows nothing of a made-up symbol', () => {
    expect(glyphUrl('nope')).toBeNull()
  })
})

describe.each(GAMES.map((g) => [g.id, g] as const))('%s badges', (_, definition) => {
  const categories: Category[] = definition.categories.filter((c) => c.id !== 'custom')

  it('only uses symbols that exist', () => {
    const icons = [...definition.categories.map((c) => c.icon), ...Object.values(definition.iconAliases ?? {})]
    for (const icon of icons) {
      const name = builtin(icon)
      if (name) expect(GLYPH_NAMES).toContain(name)
    }
  })

  it('tells every category apart by color and by symbol', () => {
    expect(new Set(categories.map((c) => c.color)).size).toBe(categories.length)
    expect(new Set(categories.map((c) => c.icon)).size).toBe(categories.length)
  })
})

describe('withIconAliases', () => {
  it('swaps listed icons only', () => {
    const pins = [{ id: 'a', icon: 'game:Old' }, { id: 'b', icon: 'game:Other' }, { id: 'c' }] as Pin[]
    expect(withIconAliases(pins, { 'game:Old': 'builtin:new' }).map((p) => p.icon)).toEqual(['builtin:new', 'game:Other', undefined])
  })
})
