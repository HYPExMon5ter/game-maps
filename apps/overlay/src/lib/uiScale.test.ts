import { describe, expect, it } from 'vitest'
import { autoUiScale, MAX_UI_SCALE, MIN_UI_SCALE } from './uiScale'

describe('autoUiScale', () => {
  it('is 1 at 1080p', () => expect(autoUiScale(1080)).toBe(1))
  it('grows on taller screens (1440p ultrawide, 4K)', () => {
    expect(autoUiScale(1440)).toBeGreaterThan(1)
    expect(autoUiScale(2160)).toBeGreaterThan(autoUiScale(1440))
  })
  it('shrinks on small screens and stays within bounds', () => {
    expect(autoUiScale(720)).toBeLessThan(1)
    expect(autoUiScale(100)).toBe(MIN_UI_SCALE)
    expect(autoUiScale(10000)).toBe(MAX_UI_SCALE)
  })
})
