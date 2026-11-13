// Walking routes to the tracked target (from the game's navigation mesh) and the
// breadcrumbs drawn along them.

import type { Category, Pin } from './game'
import type { PathInfo, RouteMove, Vec3 } from './protocol'

/**
 * Where to route to for `target`: the target itself, or, for an underground target
 * while the player is on the surface, the entrance that leads to it (the surface and
 * underground pathfinding aren't connected). Prefers an entrance whose description
 * lists the target, then the closest entrance; exits are a last resort.
 */
export function routeDestination(target: Pin, playerUnderground: boolean, pins: Pin[], categories: Category[]): Vec3 {
  if (playerUnderground || (target.layer ?? 'surface') !== 'caves') return target.position
  const entranceCategories = new Set(categories.filter((c) => c.entrance).map((c) => c.id))
  const entrances = pins.filter((p) => entranceCategories.has(p.category) && (p.layer ?? 'surface') === 'surface')
  if (entrances.length === 0) return target.position
  const isExit = (p: Pin) => /\bexit\b/i.test(p.name)
  const lists = (p: Pin) => !!p.description && p.description.toLowerCase().includes(target.name.toLowerCase())
  const score = (p: Pin) => (lists(p) ? 0 : 2) + (isExit(p) ? 1 : 0)
  const best = entrances.reduce((a, b) => {
    const sa = score(a)
    const sb = score(b)
    if (sa !== sb) return sa < sb ? a : b
    return dist(a.position, target.position) <= dist(b.position, target.position) ? a : b
  })
  return best.position
}

/** The entrance pin nearest to `p` among those `keep` accepts (the way out of a cave). */
export function nearestEntrance(p: Vec3, pins: Pin[], categories: Category[], keep: (pin: Pin) => boolean): Pin | undefined {
  const entranceCategories = new Set(categories.filter((c) => c.entrance).map((c) => c.id))
  let best: Pin | undefined
  for (const pin of pins) {
    if (!entranceCategories.has(pin.category) || !keep(pin)) continue
    if (!best || dist(pin.position, p) < dist(best.position, p)) best = pin
  }
  return best
}

const dist = (a: Vec3, b: Vec3) => Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z)
const lerp = (a: Vec3, b: Vec3, t: number): Vec3 => ({ x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t, z: a.z + (b.z - a.z) * t })

/** Closest point on segment `a`-`b` to `p`, as a fraction of the segment. */
function closestT(a: Vec3, b: Vec3, p: Vec3): number {
  const ab = { x: b.x - a.x, y: b.y - a.y, z: b.z - a.z }
  const len2 = ab.x * ab.x + ab.y * ab.y + ab.z * ab.z
  if (len2 === 0) return 0
  const t = ((p.x - a.x) * ab.x + (p.y - a.y) * ab.y + (p.z - a.z) * ab.z) / len2
  return Math.min(1, Math.max(0, t))
}

/** A breadcrumb: where it is and which way the route goes there. */
export interface Crumb {
  position: Vec3
  /** Horizontal unit vector along the route. */
  direction: Vec3
  /** Stable id: how many steps back from the route's end it sits. */
  key: number
}

/**
 * Breadcrumbs every `step` along the route over the next `ahead` units from where the
 * player is on it. They're spaced back from the route's end, so they stay put while
 * the player walks (and the route is recomputed from the player's new position).
 */
export function breadcrumbs(corners: Vec3[], player: Vec3, ahead: number, step: number): Crumb[] {
  if (corners.length < 2) return []
  const lengths = corners.slice(1).map((c, i) => dist(corners[i], c))
  const total = lengths.reduce((a, b) => a + b, 0)
  // Where the player is along the route: the route may have been computed a moment ago.
  let best = { along: 0, d: Infinity }
  let walked = 0
  for (let i = 0; i < lengths.length; i++) {
    const t = closestT(corners[i], corners[i + 1], player)
    const d = dist(lerp(corners[i], corners[i + 1], t), player)
    if (d < best.d) best = { along: walked + t * lengths[i], d }
    walked += lengths[i]
  }
  const out: Crumb[] = []
  // Steps back from the end, nearest the player first; none right under their feet.
  const first = Math.floor((total - best.along - step / 2) / step)
  let segment = 0
  let segmentStart = 0
  for (let key = first; key >= 1; key--) {
    const along = total - key * step
    if (along - best.along > ahead) continue
    while (segment < lengths.length - 1 && segmentStart + lengths[segment] < along) segmentStart += lengths[segment++]
    const a = corners[segment]
    const b = corners[segment + 1]
    const len = lengths[segment]
    const flat = Math.hypot(b.x - a.x, b.z - a.z) || 1
    out.push({
      position: lerp(a, b, len === 0 ? 0 : (along - segmentStart) / len),
      direction: { x: (b.x - a.x) / flat, y: 0, z: (b.z - a.z) / flat },
      key,
    })
  }
  return out
}

/** The line drawn to the tracked target, and how much of it can be walked. */
export interface TrackPath {
  /** From the player, along the route's corners, to the target. */
  points: Vec3[]
  /** How many legs from the start follow the walking route; the rest is a straight line. */
  walkable: number
  /** How each leg is travelled (the straight rest counts as walked). */
  moves: RouteMove[]
}

/** A route whose end is this close to the target (world units) reaches it. */
const REACHES = 3

/**
 * The line to draw from the player to the target: the walking route where there is
 * one, ending in a straight line where it stops short (an entrance on the way to an
 * underground target, a spot it can't walk to) or when there's no route at all.
 */
export function trackPath(from: Vec3, to: Vec3, route: PathInfo | null, unitsPerMeter = 1): TrackPath {
  if (!route || route.corners.length < 2) return { points: [from, to], walkable: 0, moves: ['walk'] }
  const points = [from, ...route.corners.slice(1), to]
  const last = route.corners[route.corners.length - 1]
  const reaches = route.complete && dist(last, to) <= REACHES * unitsPerMeter
  const moves = route.moves?.length === route.corners.length - 1 ? route.moves : route.corners.slice(1).map((): RouteMove => 'walk')
  return { points, walkable: reaches ? points.length - 1 : points.length - 2, moves: [...moves, 'walk'] }
}

/** The route line's color for each way of going: water blue, ropes and ziplines orange. */
export const MOVE_COLORS: Record<RouteMove, string> = {
  walk: '#ffe08a',
  swim: '#7cc8ff',
  dive: '#4f8dff',
  rope: '#ff9d4d',
  zipline: '#ff9d4d',
}

/** The legs of `path` that follow its route, in runs travelled the same way. */
export function moveRuns(path: TrackPath): { move: RouteMove; from: number; to: number }[] {
  const runs: { move: RouteMove; from: number; to: number }[] = []
  for (let i = 0; i < path.walkable; i++) {
    const last = runs[runs.length - 1]
    if (last && last.move === path.moves[i]) last.to = i + 1
    else runs.push({ move: path.moves[i], from: i, to: i + 1 })
  }
  return runs
}

/** Something on the way that isn't walking: a climb, a ride, a swim. */
export interface RouteStep {
  move: Exclude<RouteMove, 'walk'>
  /** How far along the route it starts, how long it is and how much it climbs (world units). */
  at: number
  length: number
  rise: number
}

/** The next climb, ride or swim along the route, if there's one before it ends. */
export function nextStep(path: TrackPath): RouteStep | null {
  const first = path.moves.findIndex((m, i) => m !== 'walk' && i < path.walkable)
  if (first < 0) return null
  const move = path.moves[first] as RouteStep['move']
  let last = first
  while (last + 1 < path.walkable && path.moves[last + 1] === move) last++
  const at = pathLength(path.points, first)
  return { move, at, length: pathLength(path.points, last + 1) - at, rise: path.points[last + 1].y - path.points[first].y }
}

/** Length of a polyline's first `legs` legs (all by default), heights included. */
export function pathLength(points: Vec3[], legs = points.length - 1): number {
  let total = 0
  for (let i = 1; i <= Math.min(legs, points.length - 1); i++) total += dist(points[i - 1], points[i])
  return total
}
