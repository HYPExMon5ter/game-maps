import { describe, expect, it } from 'vitest'
import { anchoredRect, anchorOf, fitRect, moveRect, resizeRect, scaleRect, toFractions, toPixels } from './layout'

const screen = { width: 1000, height: 800 }
const min = { width: 100, height: 100 }
const max = { width: 600, height: 500 }
const rect = { left: 200, top: 200, width: 300, height: 200 }

describe('moveRect', () => {
  it('follows the drag', () => {
    expect(moveRect(rect, 50, -20, screen)).toEqual({ ...rect, left: 250, top: 180 })
  })

  it('stops at the screen edges', () => {
    expect(moveRect(rect, -900, 900, screen)).toEqual({ ...rect, left: 0, top: 600 })
  })
})

describe('resizeRect', () => {
  it('moves only the dragged corner', () => {
    expect(resizeRect(rect, 'se', 50, 30, min, max, screen)).toEqual({ left: 200, top: 200, width: 350, height: 230 })
    expect(resizeRect(rect, 'nw', 50, 30, min, max, screen)).toEqual({ left: 250, top: 230, width: 250, height: 170 })
  })

  it('ignores the axis an edge handle does not move', () => {
    expect(resizeRect(rect, 'e', 50, 30, min, max, screen)).toEqual({ ...rect, width: 350 })
    expect(resizeRect(rect, 'n', 50, -30, min, max, screen)).toEqual({ ...rect, top: 170, height: 230 })
  })

  it('keeps the far edge fixed when the size limit stops the drag', () => {
    const r = resizeRect(rect, 'w', 1000, 0, min, max, screen)
    expect(r.width).toBe(100)
    expect(r.left + r.width).toBe(rect.left + rect.width)
  })

  it('stays within the size limits and the screen', () => {
    expect(resizeRect(rect, 'se', 2000, 2000, min, max, screen)).toEqual({ left: 200, top: 200, width: 600, height: 500 })
    expect(resizeRect({ ...rect, left: 700 }, 'e', 900, 0, min, max, screen).width).toBe(300)
  })
})

describe('scaleRect', () => {
  it('grows around the center', () => {
    expect(scaleRect(rect, 400, 300, screen)).toEqual({ left: 150, top: 150, width: 400, height: 300 })
  })

  it('stays on the screen', () => {
    expect(scaleRect({ ...rect, left: 0, top: 0 }, 500, 400, screen)).toEqual({ left: 0, top: 0, width: 500, height: 400 })
  })
})

describe('fractions', () => {
  it('round-trips pixels', () => {
    expect(toPixels(toFractions(rect, screen), screen)).toEqual(rect)
  })

  it('keeps a layout in place on another screen', () => {
    expect(toPixels(toFractions(rect, screen), { width: 2000, height: 1600 })).toEqual({ left: 400, top: 400, width: 600, height: 400 })
  })
})

describe('fitRect', () => {
  it('brings a layout from a bigger screen back on', () => {
    expect(fitRect({ left: 900, top: 700, width: 2000, height: 300 }, screen)).toEqual({ left: 0, top: 500, width: 1000, height: 300 })
  })
})

describe('anchors', () => {
  const size = { width: 200, height: 100 }

  it('puts the corners inside the margin', () => {
    expect(anchoredRect({ x: 0, y: 0 }, size, screen, 24)).toEqual({ left: 24, top: 24, ...size })
    expect(anchoredRect({ x: 1, y: 1 }, size, screen, 24)).toEqual({ left: 776, top: 676, ...size })
  })

  it('round-trips a dragged position', () => {
    const placed = anchoredRect({ x: 0.25, y: 0.5 }, size, screen, 24)
    expect(anchorOf(placed, screen, 24)).toEqual({ x: 0.25, y: 0.5 })
  })

  it('clamps positions past the margin', () => {
    expect(anchorOf({ left: 0, top: 790, ...size }, screen, 24)).toEqual({ x: 0, y: 1 })
  })
})
