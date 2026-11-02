import { describe, expect, it } from 'vitest'
import { glyphColor } from './badges'

describe('glyphColor', () => {
  it('uses a dark glyph on light badges and white on dark ones', () => {
    expect(glyphColor('#ffffff')).toBe('#1d232a')
    expect(glyphColor('#ffd166')).toBe('#1d232a')
    expect(glyphColor('#2d9cdb')).toBe('#ffffff')
    expect(glyphColor('#eb5757')).toBe('#ffffff')
    expect(glyphColor('not-a-color')).toBe('#ffffff')
  })
})
