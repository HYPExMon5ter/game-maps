// What a generated world adds to its pins: dungeons visited, plants growing back, and the
// locations a host can give to players who join. Pure data; see `state/worldMaps`.

import type { Category, Pin } from './game'
import type { Vec3 } from './protocol'
import type { Location } from './sync/messages'

/** How far a dungeon's door is from its pin. */
const ENTRANCE_METERS = 40
/** How far a dungeon's rooms reach from its pin, when the player teleported in. */
const INTERIOR_METERS = 160

function nearest(pins: Pin[], to: Vec3, limit: number): Pin | null {
  let best: Pin | null = null
  let distance = limit
  for (const pin of pins) {
    const d = Math.hypot(pin.position.x - to.x, pin.position.z - to.z)
    if (d < distance) {
      best = pin
      distance = d
    }
  }
  return best
}

/**
 * The dungeon the player just went into: the one by where they last stood outside, or, if
 * they didn't walk in, the closest to where they are inside.
 */
export function enteredDungeon(dungeons: Pin[], outside: Vec3 | null, inside: Vec3): Pin | null {
  return (outside && nearest(dungeons, outside, ENTRANCE_METERS)) ?? nearest(dungeons, inside, INTERIOR_METERS)
}

/** `timers` with pins growing back added, or null if there's nothing new. */
export function regrowthTimers(pins: Pin[], timers: Record<string, number>, now: number): Record<string, number> | null {
  const started = pins.filter((p) => p.respawnAt && p.respawnAt > now && timers[p.id] === undefined)
  if (started.length === 0) return null
  return { ...timers, ...Object.fromEntries(started.map((p) => [p.id, p.respawnAt as number])) }
}

/** The locations worth sending: the world's own, not what's been seen around here. */
export function sharedLocations(pins: Pin[], categories: Category[]): Location[] {
  const remembered = new Set(categories.filter((c) => c.remembered).map((c) => c.id))
  return pins.filter((p) => !p.custom && !remembered.has(p.category)).map((p) => [p.id, p.category, p.name, p.position.x, p.position.y, p.position.z])
}

/** Locations from a host as pins: only in this game's categories, it's someone else's data. */
export function hostPins(locations: Location[], categories: Category[]): Pin[] {
  const known = new Set(categories.filter((c) => !c.remembered).map((c) => c.id))
  return locations.filter(([, category]) => known.has(category)).map(([id, category, name, x, y, z]) => ({ id, category, name, position: { x, y, z } }))
}
