import { describe, expect, it } from 'vitest'
import { rejoinDelay, shouldRejoin, type RoomState } from './rejoin'

const room = (patch: Partial<RoomState>): RoomState => ({ peers: 0, players: 1, failures: 0, waitedMs: 60_000, attempts: 0, ...patch })

describe('rejoinDelay', () => {
  it('doubles up to a limit', () => {
    expect(rejoinDelay(0)).toBe(30_000)
    expect(rejoinDelay(1)).toBe(60_000)
    expect(rejoinDelay(50)).toBe(300_000)
  })
})

describe('shouldRejoin', () => {
  it('rejoins when someone is out there and nobody is connected', () => {
    expect(shouldRejoin(room({}))).toBe(true)
    expect(shouldRejoin(room({ players: 0, failures: 1 }))).toBe(true)
  })

  it('never drops friends who are connected', () => {
    expect(shouldRejoin(room({ peers: 1, players: 3 }))).toBe(false)
  })

  it('leaves a solo game alone', () => {
    expect(shouldRejoin(room({ players: 0 }))).toBe(false)
  })

  it('waits longer after each attempt', () => {
    expect(shouldRejoin(room({ waitedMs: 29_000 }))).toBe(false)
    expect(shouldRejoin(room({ waitedMs: 45_000, attempts: 1 }))).toBe(false)
  })
})
