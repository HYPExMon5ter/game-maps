import { describe, expect, it } from 'vitest'
import { cloudPixels, fractalNoise, wispyEdge } from './clouds'

describe('cloudPixels', () => {
  it('is fully opaque, so unexplored land never shows through', () => {
    const px = cloudPixels(32, 32, 16)
    for (let i = 3; i < px.length; i += 4) expect(px[i]).toBe(255)
  })

  it('has light and dark parts rather than one flat color', () => {
    const px = cloudPixels(128, 128, 32)
    const reds = [...px].filter((_, i) => i % 4 === 0)
    expect(Math.max(...reds) - Math.min(...reds)).toBeGreaterThan(60)
  })

  it('looks the same every time', () => {
    expect(cloudPixels(16, 16, 8)).toEqual(cloudPixels(16, 16, 8))
  })
})

describe('fractalNoise', () => {
  it('stays in range', () => {
    for (let i = 0; i < 500; i++) {
      const n = fractalNoise(i * 0.37, i * 0.73, 5)
      expect(n).toBeGreaterThanOrEqual(0)
      expect(n).toBeLessThanOrEqual(1)
    }
  })
})

describe('wispyEdge', () => {
  const mask = (alpha: number, count = 64) => {
    const px = new Uint8ClampedArray(count * count * 4)
    for (let i = 3; i < px.length; i += 4) px[i] = alpha
    return px
  }

  it('leaves solid fog and clear ground alone', () => {
    const solid = mask(255)
    const clear = mask(0)
    wispyEdge(solid, 64, 64, 8)
    wispyEdge(clear, 64, 64, 8)
    expect(solid).toEqual(mask(255))
    expect(clear).toEqual(mask(0))
  })

  it('breaks an even edge into thick and thin parts', () => {
    const px = mask(128)
    wispyEdge(px, 64, 64, 8)
    const alphas = [...px].filter((_, i) => i % 4 === 3)
    expect(Math.max(...alphas) - Math.min(...alphas)).toBeGreaterThan(150)
  })
})
