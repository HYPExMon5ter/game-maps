// Marks collectibles found when the player passes them (categories with `autoFound`).

import type { Category, Pin } from './game'
import type { LocalPlayer } from './protocol'
import type { Projection } from './projection'

/** How close (in meters) the player has to come, across the ground and vertically. */
export const AUTO_FOUND_RADIUS = 2.5
export const AUTO_FOUND_HEIGHT = 3

/**
 * Everything `pinsReached` goes by, to tell when asking again can't give another answer:
 * snapshots arrive many times a second, most of them with the player where they were.
 * `found` and `world` are what the caller reads the found pins and their world from.
 */
export function reachInputs(pins: Pin[], categories: Category[], found: unknown, world: unknown, local: LocalPlayer, projection: Projection, unitsPerMeter: number): unknown[] {
  const { x, y, z } = local.position
  return [x, y, z, local.layer, pins, categories, found, world, projection, unitsPerMeter]
}

export function sameInputs(a: unknown[], b: unknown[]): boolean {
  return a.length === b.length && a.every((v, i) => v === b[i])
}

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
