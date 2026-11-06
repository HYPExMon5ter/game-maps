import { describe, expect, it } from 'vitest'
import { formatEnvironment, windRotation } from './environment'

describe('formatEnvironment', () => {
  it('counts days from one', () => {
    expect(formatEnvironment({ day: 0, hour: 8, minute: 5, season: 'Summer', weather: 'Clear' })).toBe('Day 1 · 08:05 · Summer · Clear')
  })

  it('leaves out what the game does not report', () => {
    expect(formatEnvironment({ day: null, hour: 14, minute: 30, season: null, weather: null })).toBe('14:30')
  })
})

describe('windRotation', () => {
  it('points where the wind blows on a north-up map', () => {
    expect(windRotation(90, 0)).toBe(90)
  })

  it('turns with the map', () => {
    expect(windRotation(90, 90)).toBe(0)
    expect(windRotation(10, 350)).toBe(20)
    expect(windRotation(350, 10)).toBe(340)
  })
})
