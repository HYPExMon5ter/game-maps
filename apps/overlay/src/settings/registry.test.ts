import { describe, expect, it } from 'vitest'
import type { GameEntry } from '../lib/backend'
import { applicableSettings, CATEGORIES, SETTINGS, type SettingsContext } from './registry'

const game = (liveFeatures: boolean): GameEntry => ({ id: 'g', name: 'G', process: 'g.exe', steamAppId: null, defaultHotkeys: [], liveFeatures })
const ctx = (g: GameEntry | null): SettingsContext => ({ game: g, window: 'launcher', onError: () => {} })

describe('settings registry', () => {
  it('has unique ids and no empty categories', () => {
    expect(new Set(SETTINGS.map((s) => s.id)).size).toBe(SETTINGS.length)
    for (const c of CATEGORIES) expect(SETTINGS.some((s) => s.category === c.id), c.id).toBe(true)
  })

  it('shows per-game settings only with a game, and live ones only for live games', () => {
    const ids = (c: SettingsContext) => applicableSettings(c).map((s) => s.id)
    expect(ids(ctx(null))).not.toContain('map-key')
    expect(ids(ctx(null))).toContain('back-key')
    expect(ids(ctx(game(false)))).toContain('map-key')
    expect(ids(ctx(game(false)))).not.toContain('creatures')
    expect(ids(ctx(game(true)))).toEqual(expect.arrayContaining(['map-key', 'creatures', 'play-together', 'connect']))
  })
})
