// Live things on the map that can be selected, followed, tracked or teleported to:
// other players, companions (Kelvin from his GPS tracker, Virginia and other friendly
// creatures) and the game's live GPS markers (golf carts, gliders, boats, ...).

import { creatureName } from './creatures'
import { markerName } from './markers'
import type { Vec3, WorldSnapshot } from './protocol'

export interface Mover {
  id: string
  name: string
  kind: 'player' | 'companion' | 'marker'
  /** CSS color for the marker dot. */
  color: string
  position: Vec3
  /** Game icon of live markers (`icons/game/<icon>.png`). */
  icon?: string
}

export const COMPANION_COLOR = '#f2c94c'
const DEFAULT_PLAYER_COLOR = '#f472b6'
const MARKER_COLOR = '#475569'

export const MOVER_KIND_LABELS: Record<Mover['kind'], string> = { player: 'Player', companion: 'Companion', marker: 'Live marker' }

/** Other players and companions: drawn as named dots and listed in the players menu. */
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
  const named = new Set(companions.map((c) => c.name))
  const friendly: Mover[] = (snapshot.creatures ?? [])
    .filter((c) => c.kind === 'friendly')
    .map((c): Mover => ({ id: c.id, name: creatureName(c.name), kind: 'companion', color: COMPANION_COLOR, position: c.position }))
    // A companion with a GPS tracker of its own is listed once.
    .filter((c) => !named.has(c.name))
  return [...players, ...companions, ...friendly]
}

/** Every live entity: players, companions and the other live markers. */
export function liveEntities(snapshot: WorldSnapshot | null): Mover[] {
  if (!snapshot) return []
  const markers: Mover[] = (snapshot.markers ?? [])
    .filter((m) => m.kind !== 'companion')
    .map((m) => ({ id: m.id, name: markerName(m), kind: 'marker', color: MARKER_COLOR, position: m.position, icon: m.icon ?? undefined }))
  return [...movers(snapshot), ...markers]
}

export function findMover(snapshot: WorldSnapshot | null, id: string): Mover | undefined {
  return liveEntities(snapshot).find((m) => m.id === id)
}
