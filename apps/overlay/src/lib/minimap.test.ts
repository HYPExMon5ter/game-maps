import { describe, expect, it } from 'vitest'
import { minimapSize } from './minimap'

describe('minimap', () => {
  it('caps the size at half the screen', () => {
    expect(minimapSize({ width: 1600, height: 300 }, { width: 3440, height: 1440 })).toEqual({ width: 1600, height: 300 })
    expect(minimapSize({ width: 1600, height: 900 }, { width: 1920, height: 1080 })).toEqual({ width: 960, height: 540 })
  })
})
