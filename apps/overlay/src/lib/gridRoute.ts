// A* over a cave's walkable-floor grid (see `tools/mapgen/sotf_caves.py`, `nav/`).
// Pure functions so they run in a worker and in tests alike.

import type { RouteMove, Vec3 } from './protocol'

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
  /** Water nodes, one bit each (lowest first); routes may drop into water from any height. */
  water?: Uint8Array
  /** The water nodes under rock, swum underwater: up and down freely, as well as along. */
  dive?: Uint8Array
  links?: Link[]
}

/** A way between two nodes that isn't a step across the floor. */
export interface Link {
  from: number
  to: number
  kind: 'rope' | 'zipline'
}

type Move = RouteMove

const bit = (bits: Uint8Array | undefined, node: number) => !!bits && ((bits[node >> 3] >> (node & 7)) & 1) === 1

/** Whether a node is water (swum at its surface, or under it). */
export function isWater(g: CaveGrid, node: number): boolean {
  return bit(g.water, node)
}

/** Whether a node is swum underwater. */
export function isDive(g: CaveGrid, node: number): boolean {
  return bit(g.dive, node)
}

/**
 * Climbs up to this many decimeters per meter of ground are walkable (about 50°: cave
 * slopes are steep, and the player can walk them); drops up to `MAX_DROP_DM`, or any
 * height into water.
 */
const MAX_CLIMB_DM_PER_M = 12
const MAX_DROP_DM = 30
/**
 * Costs per meter, relative to walking. Climbing a rope is slow; a zipline costs its
 * length, as if walked (any less, and the search's distance estimate could pass over a
 * shorter way). Swimming, under water or on it, is the distance.
 */
const ROPE_COST = 2
const ZIPLINE_COST = 1
/** Getting on or off a rope or zipline, in meters walked. */
const LINK_OVERHEAD = 3
/** Going under, in meters: so a route doesn't bob in and out of the water for nothing. */
const DIVE_ENTRY = 2
const LINK_CODES: Record<Link['kind'], number> = { rope: 1, zipline: 2 }
const LINK_KINDS = ['rope', 'zipline'] as const
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

export function contains(g: Pick<CaveGrid, 'originX' | 'originZ' | 'cell' | 'width' | 'height'>, x: number, z: number, margin = 0): boolean {
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
  /** How each node was reached: 0 across the floor, else a link (`LINK_CODES`). */
  #via: Uint8Array
  #seen: Uint32Array
  /** Nodes already expanded in this pass (the heap may hold stale duplicates). */
  #done: Uint32Array
  #pass = 0
  #heap = new Heap()
  /** Links by the node they start from, with their cost. */
  #links = new Map<number, { to: number; code: number; cost: number }[]>()

  constructor(g: CaveGrid) {
    this.#g = g
    const n = g.floors.length
    this.#cost = new Float32Array(n)
    this.#from = new Int32Array(n)
    this.#via = new Uint8Array(n)
    this.#seen = new Uint32Array(n)
    this.#done = new Uint32Array(n)
    for (const link of g.links ?? []) {
      if (g.floors[link.from] === NONE || g.floors[link.to] === NONE) continue
      const a = nodePosition(g, link.from)
      const b = nodePosition(g, link.to)
      const cost =
        link.kind === 'rope'
          ? Math.abs(b.y - a.y) * ROPE_COST + Math.hypot(b.x - a.x, b.z - a.z)
          : Math.hypot(b.x - a.x, b.y - a.y, b.z - a.z) * ZIPLINE_COST
      const out = this.#links.get(link.from) ?? []
      out.push({ to: link.to, code: LINK_CODES[link.kind], cost: (cost + LINK_OVERHEAD) / g.cell })
      this.#links.set(link.from, out)
    }
  }

  /**
   * Walking route from `start` to `goal` (world points) that avoids the `blocked`
   * nodes, or null if there's none. With `partial`, a goal that can't be walked to
   * (under water, up a cliff, off the grid) gets a route to the nearest spot that can,
   * marked incomplete.
   */
  route(start: Vec3, goal: Vec3, blocked: ReadonlySet<number> = NO_NODES, partial = false): GridRoute | null {
    const g = this.#g
    const a = nodeAt(g, start)
    const found = nodeAt(g, goal)
    if (a === null || (found === null && !partial)) return null
    // No node to reach: search toward the goal point, ending at the closest node.
    const b = found ?? -1
    const plane = g.width * g.height
    const pass = ++this.#pass
    const cost = this.#cost
    const from = this.#from
    const seen = this.#seen
    const done = this.#done
    const via = this.#via
    const heap = this.#heap
    heap.clear()
    const bRest = b % plane
    const br = b >= 0 ? Math.floor(bRest / g.width) : (goal.z - g.originZ) / g.cell - 0.5
    const bc = b >= 0 ? bRest % g.width : (goal.x - g.originX) / g.cell - 0.5
    const goalDm = goal.y * 10
    // The nearest node to the goal reached so far (meters, height included).
    let closest = a
    let closestGap = Infinity
    const estimate = (r: number, c: number) => {
      const dr = Math.abs(r - br)
      const dc = Math.abs(c - bc)
      return Math.max(dr, dc) + (Math.SQRT2 - 1) * Math.min(dr, dc)
    }
    seen[a] = pass
    cost[a] = 0
    from[a] = -1
    via[a] = 0
    heap.push(a, 0)
    let expansions = 0
    while (heap.size > 0 && expansions++ < MAX_EXPANSIONS) {
      const node: number = heap.pop()
      if (node === b) return this.#path(b, true)
      if (done[node] === pass) continue
      done[node] = pass
      const rest: number = node % plane
      const r: number = Math.floor(rest / g.width)
      const c: number = rest % g.width
      const h: number = g.floors[node]
      if (partial) {
        const gap = Math.hypot(Math.hypot(r - br, c - bc) * g.cell, (h - goalDm) / 10)
        if (gap < closestGap) {
          closestGap = gap
          closest = node
        }
      }
      const base: number = cost[node]
      const diving = isDive(g, node)
      for (const [dc, dr, step] of DIRS) {
        const nr: number = r + dr
        const nc: number = c + dc
        if (nr < 0 || nc < 0 || nr >= g.height || nc >= g.width) continue
        for (let l = 0; l < g.layers; l++) {
          const next = l * plane + nr * g.width + nc
          const nh = g.floors[next]
          if (nh === NONE || (blocked.has(next) && next !== b)) continue
          const rise = nh - h
          const water = isWater(g, next)
          const under = isDive(g, next)
          // Underwater, swimming up or down is as easy as along.
          const free = water && (diving || under)
          if (!free && (rise > MAX_CLIMB_DM_PER_M * step * g.cell || (rise < -MAX_DROP_DM && !water))) continue
          const total = base + step + Math.abs(rise) * 0.05 + (under && !diving ? DIVE_ENTRY / g.cell : 0)
          if (done[next] === pass || (seen[next] === pass && cost[next] <= total)) continue
          seen[next] = pass
          cost[next] = total
          from[next] = node
          via[next] = 0
          heap.push(next, total + estimate(nr, nc))
        }
      }
      for (const link of this.#links.get(node) ?? NO_LINKS) {
        const next = link.to
        if (blocked.has(next) && next !== b) continue
        const total = base + link.cost
        if (done[next] === pass || (seen[next] === pass && cost[next] <= total)) continue
        seen[next] = pass
        cost[next] = total
        from[next] = node
        via[next] = link.code
        const nRest = next % plane
        heap.push(next, total + estimate(Math.floor(nRest / g.width), nRest % g.width))
      }
    }
    return partial ? this.#path(closest, false) : null
  }

  #path(goal: number, complete: boolean): GridRoute {
    const g = this.#g
    const nodes: number[] = []
    for (let n = goal; n !== -1; n = this.#from[n]) nodes.push(n)
    nodes.reverse()
    const points = nodes.map((n) => nodePosition(g, n))
    const moves = nodes.slice(1).map((n, i): Move => {
      const link = this.#via[n]
      if (link) return LINK_KINDS[link - 1]
      if (isDive(g, n) || isDive(g, nodes[i])) return 'dive'
      return isWater(g, n) ? 'swim' : 'walk'
    })
    return { ...straighten(points, moves), nodes, points, moves, complete }
  }
}

const NO_NODES: ReadonlySet<number> = new Set()
const NO_LINKS: readonly { to: number; code: number; cost: number }[] = []

/** A route on a cave grid. */
export interface GridRoute {
  /** The route straightened, for drawing. */
  corners: Vec3[]
  /** How each leg between corners is travelled. */
  cornerMoves: Move[]
  /** Every node passed through, and its floor point (about a meter apart). */
  nodes: number[]
  points: Vec3[]
  /** How each leg between points is travelled. */
  moves: Move[]
  /** False when it ends short of the goal, at the nearest spot that can be walked to. */
  complete: boolean
}

/** `simplify` for each stretch travelled the same way, so a climb or a dive keeps its ends. */
function straighten(points: Vec3[], moves: Move[]): { corners: Vec3[]; cornerMoves: Move[] } {
  const corners: Vec3[] = [points[0]]
  const cornerMoves: Move[] = []
  for (let start = 0; start < moves.length; ) {
    let end = start + 1
    while (end < moves.length && moves[end] === moves[start]) end++
    const run = simplify(points.slice(start, end + 1))
    for (let i = 1; i < run.length; i++) {
      corners.push(run[i])
      cornerMoves.push(moves[start])
    }
    start = end
  }
  return { corners, cornerMoves }
}

/** Legs of `points` that can't be walked: indices `i` for `points[i]` to `points[i + 1]`. */
export type LegCheck = (points: Vec3[]) => Promise<{ blocked: number[]; checked: number }>

/** How many times a route is searched again around newly found blocks. */
const MAX_REROUTES = 8
/** Walked legs this close (world units) to a rope's or zipline's end aren't checked. */
const LINK_CLEARANCE = 3
const isLink = (move: RouteMove | undefined) => move === 'rope' || move === 'zipline'

/**
 * A route whose every walked leg `check` confirms can be walked. Each blocked leg's far node
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
    // Where ropes and ziplines start and end: the grid puts them on the nearest floor,
    // and the walk check there meets the rock the rope hangs from, or the ledge it climbs.
    const moves = route.moves
    const linkEnds = route.points.filter((_, i) => isLink(moves[i]) || isLink(moves[i - 1]))
    const nearLink = (p: Vec3) => linkEnds.some((e) => Math.hypot(p.x - e.x, p.y - e.y, p.z - e.z) <= LINK_CLEARANCE)
    const found: number[] = []
    try {
      // Long routes take several checks: each covers what fits in a frame's budget.
      for (let from = 0; from < route.points.length - 1; ) {
        const result = await check(route.points.slice(from))
        if (result.checked === 0) break
        for (const leg of result.blocked) {
          // The check walks each leg; a climb, ride or swim isn't walked.
          if (route.moves[from + leg] !== 'walk') continue
          if (nearLink(route.points[from + leg]) || nearLink(route.points[from + leg + 1])) continue
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
