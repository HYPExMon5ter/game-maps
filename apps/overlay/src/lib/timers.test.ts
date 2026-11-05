import { describe, expect, it } from 'vitest'
import { dueTimers, formatRemaining } from './timers'

describe('dueTimers', () => {
  it('splits what is due from what still runs', () => {
    expect(dueTimers({ a: 100, b: 200, c: 300 }, 200)).toEqual({ due: ['a', 'b'], running: { c: 300 } })
  })
})

describe('formatRemaining', () => {
  it('shows what matters at each length', () => {
    expect(formatRemaining(45_000)).toBe('45s')
    expect(formatRemaining(750_000)).toBe('12m 30s')
    expect(formatRemaining(3_900_000)).toBe('1h 05m')
  })

  it('rounds up and never goes negative', () => {
    expect(formatRemaining(100)).toBe('1s')
    expect(formatRemaining(-5)).toBe('0s')
  })
})
