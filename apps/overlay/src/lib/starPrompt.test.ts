import { describe, expect, it } from 'vitest'
import { STAR_DELAY_MS, starDue } from './starPrompt'

describe('starDue', () => {
  it('waits for five minutes of running', () => {
    expect(starDue(STAR_DELAY_MS - 1, false)).toBe(false)
    expect(starDue(STAR_DELAY_MS, false)).toBe(true)
  })

  it('comes sooner once a game has been played', () => {
    expect(starDue(1000, true)).toBe(true)
  })
})
