import { describe, expect, it, vi } from 'vitest'

vi.mock('./backend', () => ({ backend: {} }))

const { allowReload, repeatFilter } = await import('./errors')

describe('repeatFilter', () => {
  it('holds back repeats and says how many there were', () => {
    const filter = repeatFilter()
    expect(filter('boom', 0)).toBe('boom')
    expect(filter('boom', 1000)).toBeNull()
    expect(filter('boom', 2000)).toBeNull()
    expect(filter('boom', 11_000)).toBe('boom (and 2 more like it)')
  })

  it('treats different messages separately', () => {
    const filter = repeatFilter()
    expect(filter('a', 0)).toBe('a')
    expect(filter('b', 1)).toBe('b')
  })
})

describe('allowReload', () => {
  it('allows a few reloads, then stops', () => {
    expect(allowReload([], 1000)).toEqual([1000])
    expect(allowReload([1, 2], 3)).toEqual([1, 2, 3])
    expect(allowReload([1, 2, 3], 4)).toBeNull()
  })

  it('forgets old reloads', () => {
    expect(allowReload([0, 1, 2], 11 * 60_000)).toEqual([11 * 60_000])
  })
})
