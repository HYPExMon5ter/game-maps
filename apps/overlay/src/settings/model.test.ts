import { describe, expect, it } from 'vitest'
import { DEFAULT_GAME_SETTINGS, DEFAULT_SETTINGS, gameSettings, migrateHud, readSettings, withGameSettings } from './model'

describe('settings model', () => {
  it('fills in defaults', () => {
    expect(readSettings(undefined, ['a'])).toEqual({ settings: DEFAULT_SETTINGS, legacyHotkey: null })
    expect(readSettings({ opacity: 0.5 }, []).settings).toEqual({ ...DEFAULT_SETTINGS, opacity: 0.5 })
  })

  it('gives each game the per-game settings older versions kept for all games', () => {
    const { settings } = readSettings({ autoFound: false, playTogether: false, units: 'metric' }, ['a', 'b'])
    expect(settings.units).toBe('metric')
    expect(settings.games).toEqual({ a: { autoFound: false, playTogether: false }, b: { autoFound: false, playTogether: false } })
    expect(settings).not.toHaveProperty('autoFound')
  })

  it('keeps a game’s own settings over the old shared ones', () => {
    const { settings } = readSettings({ showAnimals: false, games: { a: { showAnimals: true } } }, ['a', 'b'])
    expect(settings.games).toEqual({ a: { showAnimals: true }, b: { showAnimals: false } })
  })

  it('drops settings that moved elsewhere, returning the old map hotkey', () => {
    const hotkey = { action: 'toggle_map' as const, vk: 77, ctrl: false, shift: false, alt: false }
    const { settings, legacyHotkey } = readSettings({ hotkey, followPlayer: true }, [])
    expect(legacyHotkey).toEqual(hotkey)
    expect(settings).toEqual(DEFAULT_SETTINGS)
  })

  it('reads and changes one game’s settings', () => {
    const s = withGameSettings(DEFAULT_SETTINGS, 'a', { showCreatures: false })
    expect(gameSettings(s, 'a')).toEqual({ ...DEFAULT_GAME_SETTINGS, showCreatures: false })
    expect(gameSettings(s, 'b')).toEqual(DEFAULT_GAME_SETTINGS)
    expect(gameSettings(s, null)).toEqual(DEFAULT_GAME_SETTINGS)
  })

  it('turns the old square mini-map size into width and height', () => {
    expect(migrateHud({ size: 420, opacity: 0.5 })).toEqual({ ...DEFAULT_SETTINGS.hud, width: 420, height: 420, opacity: 0.5 })
    expect(migrateHud(undefined)).toEqual(DEFAULT_SETTINGS.hud)
    expect(migrateHud({ width: 500, height: 250 })).toMatchObject({ width: 500, height: 250, zoom: 0 })
  })
})
