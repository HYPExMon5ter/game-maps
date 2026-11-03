import { describe, expect, it } from 'vitest'
import { findMover, liveEntities, movers } from './movers'
import type { WorldSnapshot } from './protocol'

const snap: WorldSnapshot = {
  frame: 1,
  local: null,
  others: [{ id: 'p1', name: 'Alex', color: [255, 0, 0], position: { x: 1, y: 2, z: 3 }, forward: null }],
  markers: [
    { id: 'gps:1', kind: 'companion', label: 'Kelvin', icon: 'RobbyLogo', position: { x: 4, y: 5, z: 6 } },
    { id: 'gps:2', kind: 'locator', label: null, icon: 'GolfCartIcon', position: { x: 7, y: 8, z: 9 } },
  ],
}

describe('movers', () => {
  it('lists players and companions, not other markers', () => {
    expect(movers(snap).map((m) => [m.id, m.name, m.kind])).toEqual([
      ['p1', 'Alex', 'player'],
      ['gps:1', 'Kelvin', 'companion'],
    ])
  })
  it('finds companions by id for tracking and teleports', () => {
    expect(findMover(snap, 'gps:1')?.position).toEqual({ x: 4, y: 5, z: 6 })
    expect(findMover(null, 'gps:1')).toBeUndefined()
  })
  it('includes live markers among live entities, named from their icon', () => {
    expect(liveEntities(snap).map((m) => [m.id, m.name, m.kind])).toEqual([
      ['p1', 'Alex', 'player'],
      ['gps:1', 'Kelvin', 'companion'],
      ['gps:2', 'Golf Cart', 'marker'],
    ])
    expect(findMover(snap, 'gps:2')?.icon).toBe('GolfCartIcon')
  })
})
