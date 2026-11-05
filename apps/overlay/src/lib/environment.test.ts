import { describe, expect, it } from 'vitest'
import { formatEnvironment } from './environment'

describe('formatEnvironment', () => {
  it('counts days from one', () => {
    expect(formatEnvironment({ day: 0, hour: 8, minute: 5, season: 'Summer', weather: 'Clear' })).toBe('Day 1 · 08:05 · Summer · Clear')
  })

  it('leaves out what the game does not report', () => {
    expect(formatEnvironment({ day: null, hour: 14, minute: 30, season: null, weather: null })).toBe('14:30')
  })
})
