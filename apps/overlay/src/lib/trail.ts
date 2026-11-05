// Path history: where the player has walked, as lines on the map. Pure data; see
// `state/trail` for recording.

import { groundDistance, type Projection } from './projection'
import type { LocalPlayer, Vec3, WorldLayer } from './protocol'

/** A new point is kept once the player has moved this far (meters). */
const STEP_METERS = 5
/** A jump this big (meters) is a teleport or a load: the line breaks there. */
const BREAK_METERS = 60
/** How many points are kept; the oldest go first (about 25 km of walking). */
const MAX_POINTS = 5000

export interface TrailPoint {
  position: Vec3
  layer: WorldLayer
  /** Starts a new line (nothing connects it to the point before). */
  start: boolean
}

/** `trail` with the player's position added, or `trail` itself if it needs no new point. */
export function extendTrail(trail: TrailPoint[], local: LocalPlayer, projection: Projection, unitsPerMeter: number): TrailPoint[] {
  const last = trail.at(-1)
  const moved = last ? groundDistance(projection, last.position, local.position) / unitsPerMeter : Infinity
  const changedLayer = last !== undefined && last.layer !== local.layer
  if (moved < STEP_METERS && !changedLayer) return trail
  const point = { position: local.position, layer: local.layer, start: !last || moved > BREAK_METERS || changedLayer }
  return [...trail.slice(-(MAX_POINTS - 1)), point]
}

/** The lines to draw on `layer`. */
export function trailLines(trail: TrailPoint[], layer: WorldLayer): Vec3[][] {
  const lines: Vec3[][] = []
  for (const p of trail) {
    if (p.layer !== layer) continue
    if (p.start || lines.length === 0) lines.push([p.position])
    else lines.at(-1)!.push(p.position)
  }
  return lines.filter((l) => l.length >= 2)
}
