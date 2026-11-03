// A* over a cave's walkable-floor grid (see `tools/mapgen/sotf_caves.py`, `nav/`).
// Pure functions so they run in a worker and in tests alike.

import type { Vec3 } from './protocol'

export const NONE = -32768

/** One cave's grid: floor heights in decimeters per layer, row (south first), column. */
export interface CaveGrid {
  area: number
  originX: number
  originZ: number
  cell: number
  width: number
  height: number
  layers: number
  floors: Int16Array
}

/**
 * Climbs up to this many decimeters per meter of ground are walkable (about 50°: cave
 * slopes are steep, and the player can walk them); drops up to `MAX_DROP_DM`.
 */
const MAX_CLIMB_DM_PER_M = 12
const MAX_DROP_DM = 30
/** How far (cells) from a point to look for walkable floor when it's off the grid. */
const SNAP_CELLS = 6
/** Search budget: big caves have a few million cells; routes need far fewer. */
const MAX_EXPANSIONS = 2_000_000

const DIRS = [
  [1, 0, 1],
  [-1, 0, 1],
  [0, 1, 1],
  [0, -1, 1],
  [1, 1, Math.SQRT2],
  [1, -1, Math.SQRT2],
  [-1, 1, Math.SQRT2],
  [-1, -1, Math.SQRT2],
] as const

export function contains(g: CaveGrid, x: number, z: number, margin = 0): boolean {
  return x >= g.originX - margin && z >= g.originZ - margin && x <= g.originX + g.width * g.cell + margin && z <= g.originZ + g.height * g.cell + margin
}

/** Walkable node nearest to a world point (preferring the floor nearest its height). */
export function nodeAt(g: CaveGrid, p: Vec3): number | null {
  const c0 = Math.floor((p.x - g.originX) / g.cell)
  const r0 = Math.floor((p.z - g.originZ) / g.cell)
  const plane = g.width * g.height
  let best: { node: number; score: number } | null = null
  for (let dr = -SNAP_CELLS; dr <= SNAP_CELLS; dr++) {
    for (let dc = -SNAP_CELLS; dc <= SNAP_CELLS; dc++) {
      const r = r0 + dr
      const c = c0 + dc
      if (r < 0 || c < 0 || r >= g.height || c >= g.width) continue
      for (let l = 0; l < g.layers; l++) {
        const h = g.floors[l * plane + r * g.width + c]
        if (h === NONE) continue
        const score = Math.hypot(dr, dc) * g.cell + Math.abs(h / 10 - p.y) * 0.5
        if (!best || score < best.score) best = { node: l * plane + r * g.width + c, score }
      }
    }
  }
  return best?.node ?? null
}

export function nodePosition(g: CaveGrid, node: number): Vec3 {
  const plane = g.width * g.height
  const rest = node % plane
  const r = Math.floor(rest / g.width)
  const c = rest % g.width
  return { x: g.originX + (c + 0.5) * g.cell, y: g.floors[node] / 10, z: g.originZ + (r + 0.5) * g.cell }
}

/** Reusable search buffers for one grid (allocating millions of entries per search is slow). */
export class GridSearch {
  #g: CaveGrid
  #cost: Float32Array
  #from: Int32Array
  #seen: Uint32Array
  /** Nodes already expanded in this pass (the heap may hold stale duplicates). */
  #done: Uint32Array
  #pass = 0
  #heap = new Heap()

  constructor(g: CaveGrid) {
    this.#g = g
    const n = g.floors.length
    this.#cost = new Float32Array(n)
    this.#from = new Int32Array(n)
    this.#seen = new Uint32Array(n)
    this.#done = new Uint32Array(n)
  }

  /**
   * Walking route from `start` to `goal` (world points) that avoids the `blocked`
   * nodes, or null if there's none.
   */
  route(start: Vec3, goal: Vec3, blocked: ReadonlySet<number> = NO_NODES): GridRoute | null {
    const g = this.#g
    const a = nodeAt(g, start)
    const b = nodeAt(g, goal)
    if (a === null || b === null) return null
    const plane = g.width * g.height
    const pass = ++this.#pass
    const cost = this.#cost
    const from = this.#from
    const seen = this.#seen
    const done = this.#done
    const heap = this.#heap
    heap.clear()
    const bRest = b % plane
    const br = Math.floor(bRest / g.width)
    const bc = bRest % g.width
    const estimate = (r: number, c: number) => {
      const dr = Math.abs(r - br)
      const dc = Math.abs(c - bc)
      return Math.max(dr, dc) + (Math.SQRT2 - 1) * Math.min(dr, dc)
    }
    seen[a] = pass
    cost[a] = 0
    from[a] = -1
    heap.push(a, 0)
    let expansions = 0
    while (heap.size > 0 && expansions++ < MAX_EXPANSIONS) {
      const node: number = heap.pop()
      if (node === b) return this.#path(b)
      if (done[node] === pass) continue
      done[node] = pass
      const rest: number = node % plane
      const r: number = Math.floor(rest / g.width)
      const c: number = rest % g.width
      const h: number = g.floors[node]
      const base: number = cost[node]
      for (const [dc, dr, step] of DIRS) {
        const nr: number = r + dr
        const nc: number = c + dc
        if (nr < 0 || nc < 0 || nr >= g.height || nc >= g.width) continue
        for (let l = 0; l < g.layers; l++) {
          const next = l * plane + nr * g.width + nc
          const nh = g.floors[next]
          if (nh === NONE || (blocked.has(next) && next !== b)) continue
          const rise = nh - h
          if (rise > MAX_CLIMB_DM_PER_M * step * g.cell || rise < -MAX_DROP_DM) continue
          const total = base + step + Math.abs(rise) * 0.05
          if (done[next] === pass || (seen[next] === pass && cost[next] <= total)) continue
          seen[next] = pass
          cost[next] = total
          from[next] = node
          heap.push(next, total + estimate(nr, nc))
        }
      }
    }
    return null
  }

  #path(goal: number): GridRoute {
    const nodes: number[] = []
    for (let n = goal; n !== -1; n = this.#from[n]) nodes.push(n)
    nodes.reverse()
    const points = nodes.map((n) => nodePosition(this.#g, n))
    return { corners: simplify(points), nodes, points }
  }
}

const NO_NODES: ReadonlySet<number> = new Set()

/** A route on a cave grid. */
export interface GridRoute {
  /** The route straightened, for drawing. */
  corners: Vec3[]
  /** Every node walked through, and its floor point (about a meter apart). */
  nodes: number[]
  points: Vec3[]
}

/** Legs of `points` that can't be walked: indices `i` for `points[i]` to `points[i + 1]`. */
export type LegCheck = (points: Vec3[]) => Promise<{ blocked: number[]; checked: number }>

/** How many times a route is searched again around newly found blocks. */
const MAX_REROUTES = 8

/**
 * A route whose every leg `check` confirms can be walked. Each blocked leg's far node
 * is added to `blocked` (with `now()`), and the route is searched again around it.
 * Legs into the destination are never blocked: there's nowhere else to go. Without a
 * working check, the unchecked route is the best there is; after `MAX_REROUTES`, so is
 * the last one tried. Null only when there's no way around the blocks.
 */
export async function walkableRoute(
  find: (blocked: ReadonlySet<number>) => Promise<GridRoute | null>,
  check: LegCheck,
  blocked: Map<number, number>,
  now: () => number = Date.now,
): Promise<GridRoute | null> {
  let route: GridRoute | null = null
  for (let attempt = 0; attempt <= MAX_REROUTES; attempt++) {
    route = await find(new Set(blocked.keys()))
    if (!route) return null
    const goal = route.nodes[route.nodes.length - 1]
    const found: number[] = []
    try {
      // Long routes take several checks: each covers what fits in a frame's budget.
      for (let from = 0; from < route.points.length - 1; ) {
        const result = await check(route.points.slice(from))
        if (result.checked === 0) break
        for (const leg of result.blocked) {
          const node = route.nodes[from + leg + 1]
          if (node !== goal) found.push(node)
        }
        from += result.checked
      }
    } catch {
      return route
    }
    if (found.length === 0) return route
    for (const node of found) blocked.set(node, now())
  }
  return route
}

/** How far (meters) a cell-by-cell route may stray when straightened. */
const SIMPLIFY_TOLERANCE = 0.75

/**
 * Straightens a cell-by-cell route (Ramer-Douglas-Peucker): drops points within
 * `SIMPLIFY_TOLERANCE` of the line between the points kept around them.
 */
export function simplify(points: Vec3[], tolerance = SIMPLIFY_TOLERANCE): Vec3[] {
  if (points.length <= 2) return points
  const keep = new Uint8Array(points.length)
  keep[0] = keep[points.length - 1] = 1
  const stack: [number, number][] = [[0, points.length - 1]]
  while (stack.length) {
    const [i, j] = stack.pop()!
    let worst = -1
    let worstDist = tolerance
    for (let k = i + 1; k < j; k++) {
      const d = distanceToSegment(points[k], points[i], points[j])
      if (d > worstDist) {
        worst = k
        worstDist = d
      }
    }
    if (worst >= 0) {
      keep[worst] = 1
      stack.push([i, worst], [worst, j])
    }
  }
  return points.filter((_, i) => keep[i])
}

function distanceToSegment(p: Vec3, a: Vec3, b: Vec3): number {
  const ab = { x: b.x - a.x, y: b.y - a.y, z: b.z - a.z }
  const len2 = ab.x * ab.x + ab.y * ab.y + ab.z * ab.z
  const t = len2 === 0 ? 0 : Math.min(1, Math.max(0, ((p.x - a.x) * ab.x + (p.y - a.y) * ab.y + (p.z - a.z) * ab.z) / len2))
  return Math.hypot(p.x - (a.x + ab.x * t), p.y - (a.y + ab.y * t), p.z - (a.z + ab.z * t))
}

/** Binary min-heap of node ids by priority. */
class Heap {
  #ids = new Int32Array(1024)
  #keys = new Float32Array(1024)
  size = 0

  clear() {
    this.size = 0
  }

  push(id: number, key: number) {
    if (this.size === this.#ids.length) {
      const ids = new Int32Array(this.size * 2)
      const keys = new Float32Array(this.size * 2)
      ids.set(this.#ids)
      keys.set(this.#keys)
      this.#ids = ids
      this.#keys = keys
    }
    let i = this.size++
    while (i > 0) {
      const p = (i - 1) >> 1
      if (this.#keys[p] <= key) break
      this.#ids[i] = this.#ids[p]
      this.#keys[i] = this.#keys[p]
      i = p
    }
    this.#ids[i] = id
    this.#keys[i] = key
  }

  pop(): number {
    const top = this.#ids[0]
    const lastId = this.#ids[--this.size]
    const lastKey = this.#keys[this.size]
    let i = 0
    for (;;) {
      let child = 2 * i + 1
      if (child >= this.size) break
      if (child + 1 < this.size && this.#keys[child + 1] < this.#keys[child]) child++
      if (this.#keys[child] >= lastKey) break
      this.#ids[i] = this.#ids[child]
      this.#keys[i] = this.#keys[child]
      i = child
    }
    this.#ids[i] = lastId
    this.#keys[i] = lastKey
    return top
  }
}
