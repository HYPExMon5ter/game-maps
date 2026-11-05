import { describe, expect, it } from 'vitest'
import { emptyFog, exploredAt, reveal, saveFog } from '../lib/fog'
import type { Projection } from '../lib/projection'
import { mergeFriendFog, useFogStore } from './fog'

const p: Projection = { east: 'x', north: 'z', up: 'y', minEast: -400, maxEast: 400, minNorth: -400, maxNorth: 400 }

describe('mergeFriendFog', () => {
  it('adds what a friend explored', () => {
    const mine = emptyFog(p, 1)
    const theirs = emptyFog(p, 1)
    reveal(theirs, 200, 0, 40)
    useFogStore.setState({ key: 'k', fog: mine })
    mergeFriendFog(saveFog(theirs))
    expect(exploredAt(mine, 200, 0)).toBe(true)
  })
})
