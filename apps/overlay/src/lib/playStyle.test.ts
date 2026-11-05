import { describe, expect, it } from 'vitest'
import { DEFAULT_SETTINGS, readSettings } from '../settings/model'
import { PLAY_STYLES, withPlayStyle } from './playStyle'

const style = (id: string) => PLAY_STYLES.find((s) => s.id === id)!

describe('withPlayStyle', () => {
  it('sets the fog and teleport of each style', () => {
    expect(withPlayStyle(DEFAULT_SETTINGS, style('discover'))).toMatchObject({ fogOfWar: true, teleport: false })
    expect(withPlayStyle(DEFAULT_SETTINGS, style('full'))).toMatchObject({ fogOfWar: false, teleport: false })
    expect(withPlayStyle(DEFAULT_SETTINGS, style('teleport'))).toMatchObject({ fogOfWar: false, teleport: true })
  })

  it('asks only once', () => {
    expect(withPlayStyle(DEFAULT_SETTINGS, style('full')).onboarding.welcomed).toBe(true)
  })
})

describe('first run', () => {
  it('asks a new player', () => {
    const { settings } = readSettings(null, [])
    expect(settings.onboarding).toEqual({ welcomed: false, fogTip: false, star: false })
  })

  it("doesn't ask someone who already has settings, and leaves their teleport on", () => {
    const { settings } = readSettings({ opacity: 0.5 }, [])
    expect(settings.onboarding).toEqual({ welcomed: true, fogTip: true, star: false })
    expect(settings.teleport).toBe(true)
  })

  it('remembers what was already answered', () => {
    const { settings } = readSettings({ onboarding: { welcomed: true, fogTip: false, star: true } }, [])
    expect(settings.onboarding).toEqual({ welcomed: true, fogTip: false, star: true })
  })
})
