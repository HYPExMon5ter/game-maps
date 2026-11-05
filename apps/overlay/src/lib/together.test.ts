import { describe, expect, it } from 'vitest'
import { connectionOf } from './together'

describe('connectionOf', () => {
  it('matches players to peers by name', () => {
    const peers = [
      { id: 'a', name: 'Ana', ready: true },
      { id: 'b', name: 'Bo', ready: false },
    ]
    expect(connectionOf({ name: 'Ana' }, peers)).toBe('connected')
    expect(connectionOf({ name: 'Bo' }, peers)).toBe('outdated')
    expect(connectionOf({ name: 'Cy' }, peers)).toBe('not-connected')
  })

  it('does not count a peer that has not said who it is', () => {
    expect(connectionOf({ name: 'Ana' }, [{ id: 'a', name: null, ready: false }])).toBe('not-connected')
  })
})
