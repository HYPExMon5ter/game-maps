// The closest thing the player hasn't found yet.

import type { Category, Pin } from './game'
import { groundDistance, type Projection } from './projection'
import type { LocalPlayer } from './protocol'

/**
 * The nearest game pin not found yet among `categories`, on the player's layer when
 * there's one there. `hidden` leaves out what the player shouldn't see (fog of war).
 */
export function nearestUnfound(pins: Pin[], categories: Category[], found: Iterable<string>, local: LocalPlayer, projection: Projection, hidden?: (pin: Pin) => boolean): Pin | null {
  const isFound = new Set(found)
  const wanted = new Set(categories.map((c) => c.id))
  const layer = local.layer === 'caves' ? 'caves' : 'surface'
  const candidates = pins.filter((p) => !p.custom && wanted.has(p.category) && !isFound.has(p.id) && !hidden?.(p))
  const here = candidates.filter((p) => (p.layer ?? 'surface') === layer)
  let best: Pin | null = null
  let bestDistance = Infinity
  for (const pin of here.length ? here : candidates) {
    const d = groundDistance(projection, local.position, pin.position)
    if (d < bestDistance) {
      best = pin
      bestDistance = d
    }
  }
  return best
}
