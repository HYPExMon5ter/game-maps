// Routes inside caves from the map pack's navigation grids (`nav/caves.json`), for games
// whose own pathfinding doesn't cover caves. The search runs in a worker.

import type { CaveInfo, CaveRequest, CaveResponse } from './caveNav.worker'
import { ASSET_BASE } from './game'
import { contains, walkableRoute, type GridRoute, type LegCheck } from './gridRoute'
import type { PathInfo, Vec3 } from './protocol'

const indexes = new Map<string, Promise<CaveInfo[]>>()

/** The caves with navigation grids for a game (empty if its map pack has none). */
export function caveIndex(gameId: string): Promise<CaveInfo[]> {
  let index = indexes.get(gameId)
  if (!index) {
    index = fetch(`${navBase(gameId)}/caves.json`)
      .then((r) => (r.ok ? (r.json() as Promise<{ caves: CaveInfo[] }>) : { caves: [] }))
      .then((d) => d.caves)
      .catch(() => [])
    indexes.set(gameId, index)
  }
  return index
}

function navBase(gameId: string) {
  return `${ASSET_BASE}/games/${encodeURIComponent(gameId)}/nav`
}

/** The cave whose grid covers a point (smallest first: caves can sit inside others' bounds). */
export function caveAt(caves: CaveInfo[], p: Vec3, margin = 2): CaveInfo | undefined {
  return caves
    .filter((c) => contains(c, p.x, p.z, margin))
    .sort((a, b) => a.width * a.height - b.width * b.height)[0]
}

let worker: Worker | null = null
let nextId = 1
const pending = new Map<number, (r: CaveResponse) => void>()

function getWorker(): Worker {
  if (!worker) {
    worker = new Worker(new URL('./caveNav.worker.ts', import.meta.url), { type: 'module' })
    worker.onmessage = (e: MessageEvent<CaveResponse>) => {
      pending.get(e.data.id)?.(e.data)
      pending.delete(e.data.id)
    }
  }
  return worker
}

/** Walking route inside `cave` around `blocked` nodes, or null when there's no way through. */
function gridRoute(gameId: string, cave: CaveInfo, from: Vec3, to: Vec3, blocked: ReadonlySet<number>): Promise<GridRoute | null> {
  const id = nextId++
  return new Promise((resolve) => {
    pending.set(id, (r) => resolve(r.route))
    const request: CaveRequest = { id, baseUrl: navBase(gameId), cave, from, to, blocked: [...blocked] }
    getWorker().postMessage(request)
  })
}

/**
 * Grid nodes found blocked in the game, per cave grid, with when. They expire so that
 * a passage reopens once its blocker is gone (blown up, moved).
 */
const blockedNodes = new Map<string, Map<number, number>>()
const BLOCK_TTL_MS = 45_000

/**
 * Walking route inside `cave`, checked against the game world as it is now with
 * `check`: blocked spots are routed around. When the goal can't be walked to, the route
 * ends at the nearest spot that can (`complete: false`). Null when there's no way at all.
 */
export async function caveRoute(gameId: string, cave: CaveInfo, from: Vec3, to: Vec3, check: LegCheck): Promise<PathInfo | null> {
  const key = `${gameId}/${cave.file}`
  let blocked = blockedNodes.get(key)
  if (!blocked) blockedNodes.set(key, (blocked = new Map()))
  const now = Date.now()
  for (const [node, at] of blocked) if (now - at > BLOCK_TTL_MS) blocked.delete(node)
  const route = await walkableRoute((avoid) => gridRoute(gameId, cave, from, to, avoid), check, blocked)
  // A route that doesn't get anywhere is no route.
  if (!route || route.nodes.length < 2) return null
  return { corners: route.corners, complete: route.complete, moves: route.cornerMoves }
}
