import { describe, expect, it } from 'vitest'
import { DEFAULT_SETTINGS } from '../state/store'
import { minimapSize, migrateHud } from './minimap'

describe('minimap', () => {
  it('caps the size at half the screen', () => {
    expect(minimapSize({ width: 1600, height: 300 }, { width: 3440, height: 1440 })).toEqual({ width: 1600, height: 300 })
    expect(minimapSize({ width: 1600, height: 900 }, { width: 1920, height: 1080 })).toEqual({ width: 960, height: 540 })
  })

  it('turns the old square size into width and height', () => {
    expect(migrateHud({ size: 420, opacity: 0.5 })).toEqual({ ...DEFAULT_SETTINGS.hud, width: 420, height: 420, opacity: 0.5 })
    expect(migrateHud(undefined)).toEqual(DEFAULT_SETTINGS.hud)
    expect(migrateHud({ width: 500, height: 250 })).toMatchObject({ width: 500, height: 250, zoom: 0 })
  })
})
