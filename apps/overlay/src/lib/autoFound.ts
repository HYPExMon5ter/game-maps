// Marks collectibles found when the player passes them (categories with `autoFound`).

import type { Category, Pin } from './game'
import type { LocalPlayer } from './protocol'
import type { Projection } from './projection'

/** How close (in meters) the player has to come, across the ground and vertically. */
export const AUTO_FOUND_RADIUS = 2.5
export const AUTO_FOUND_HEIGHT = 3

/** Pins in auto-found categories, not yet found, that the player is standing at. */
export function pinsReached(
  pins: Pin[],
  categories: Category[],
  found: ReadonlySet<string>,
  local: LocalPlayer,
  projection: Projection,
  unitsPerMeter: number,
): Pin[] {
  const auto = new Set(categories.filter((c) => c.autoFound).map((c) => c.id))
  const radius = AUTO_FOUND_RADIUS * unitsPerMeter
  const height = AUTO_FOUND_HEIGHT * unitsPerMeter
  const { east, north, up } = projection
  const p = local.position
  return pins.filter((pin) => {
    if (!auto.has(pin.category) || pin.custom || found.has(pin.id)) return false
    if ((pin.layer ?? 'surface') !== (local.layer === 'caves' ? 'caves' : 'surface')) return false
    const q = pin.position
    return Math.hypot(q[east] - p[east], q[north] - p[north]) <= radius && Math.abs(q[up] - p[up]) <= height
  })
}
