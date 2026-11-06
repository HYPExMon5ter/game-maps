import { describe, expect, it } from 'vitest'
import { imageScale, maskScale } from './fogCanvas'

describe('maskScale', () => {
  it('is a pixel per cell on ordinary maps', () => {
    expect(maskScale({ width: 500, height: 480 })).toBe(1)
  })

  it('halves on the largest maps', () => {
    expect(maskScale({ width: 2048, height: 2048 })).toBe(0.5)
  })
})

describe('imageScale', () => {
  it('keeps full detail on ordinary maps', () => {
    expect(imageScale({ width: 500, height: 480 })).toBe(2)
  })

  it('holds huge maps to one image size', () => {
    expect(imageScale({ width: 2048, height: 2048 })).toBe(1)
  })
})
