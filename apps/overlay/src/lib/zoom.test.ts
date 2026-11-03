import { describe, expect, it } from 'vitest'
import { steppedZoom, ZOOM_STEP } from './zoom'

describe('steppedZoom', () => {
  it('steps in both directions', () => {
    expect(steppedZoom(3, 1, 0, 8)).toBe(3 + ZOOM_STEP)
    expect(steppedZoom(3, -2, 0, 8)).toBe(3 - 2 * ZOOM_STEP)
  })

  it('stays within the limits', () => {
    expect(steppedZoom(7.9, 1, 0, 8)).toBe(8)
    expect(steppedZoom(0.1, -1, 0, 8)).toBe(0)
  })
})
