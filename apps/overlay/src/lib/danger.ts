// Danger alert: hostile creatures close to the player (see `ui/OverGame`).

import type { Creature, WorldSnapshot } from './protocol'

export interface Threat {
  creature: Creature
  /** Distance in world units. */
  distance: number
}

/** Hostiles within `radius` world units of the player, nearest first, at most `max`. */
export function nearbyHostiles(snapshot: WorldSnapshot | null, radius: number, max = 5): Threat[] {
  const p = snapshot?.local?.position
  if (!p || !snapshot?.creatures?.length) return []
  const out: Threat[] = []
  for (const creature of snapshot.creatures) {
    if (creature.kind !== 'hostile') continue
    const c = creature.position
    const distance = Math.hypot(c.x - p.x, c.y - p.y, c.z - p.z)
    if (distance <= radius) out.push({ creature, distance })
  }
  return out.sort((a, b) => a.distance - b.distance).slice(0, max)
}
