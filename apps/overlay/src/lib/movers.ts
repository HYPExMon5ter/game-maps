// Things that move and can be tracked or teleported to: other players and companions
// (e.g. Kelvin) the game tracks on its GPS.

import type { Vec3, WorldSnapshot } from './protocol'

export interface Mover {
  id: string
  name: string
  kind: 'player' | 'companion'
  /** CSS color for the marker dot. */
  color: string
  position: Vec3
}

export const COMPANION_COLOR = '#f2c94c'
const DEFAULT_PLAYER_COLOR = '#f472b6'

export function movers(snapshot: WorldSnapshot | null): Mover[] {
  if (!snapshot) return []
  const players: Mover[] = snapshot.others.map((o) => ({
    id: o.id,
    name: o.name,
    kind: 'player',
    color: o.color ? `rgb(${o.color.join(',')})` : DEFAULT_PLAYER_COLOR,
    position: o.position,
  }))
  const companions: Mover[] = (snapshot.markers ?? [])
    .filter((m) => m.kind === 'companion')
    .map((m) => ({ id: m.id, name: m.label ?? 'Companion', kind: 'companion', color: COMPANION_COLOR, position: m.position }))
  return [...players, ...companions]
}

export function findMover(snapshot: WorldSnapshot | null, id: string): Mover | undefined {
  return movers(snapshot).find((m) => m.id === id)
}
