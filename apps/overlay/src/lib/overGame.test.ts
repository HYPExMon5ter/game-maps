import { describe, expect, it } from 'vitest'
import { pointsNear } from './overGame'

describe('pointsNear', () => {
  const rect = { left: 100, top: 50, width: 200, height: 100 }

  it('finds points on the rectangle or within the padding', () => {
    expect(pointsNear([{ x: 150, y: 100 }], rect, 0)).toBe(true)
    expect(pointsNear([{ x: 90, y: 100 }], rect, 20)).toBe(true)
    expect(pointsNear([{ x: 90, y: 100 }], rect, 5)).toBe(false)
    expect(pointsNear([{ x: 500, y: 500 }, { x: 310, y: 40 }], rect, 20)).toBe(true)
    expect(pointsNear([], rect, 20)).toBe(false)
  })
})
