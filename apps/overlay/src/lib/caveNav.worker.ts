// Routes inside caves, off the UI thread (see `caveNav.ts`). Grids load once per cave
// and keep their search buffers between requests.

import { GridSearch, type CaveGrid, type GridRoute, type Link } from './gridRoute'
import type { Vec3 } from './protocol'

export interface CaveInfo {
  area: number
  originX: number
  originZ: number
  cell: number
  width: number
  height: number
  layers: number
  file: string
  /** Water nodes (see `CaveGrid.water`), when the cave has water. */
  water?: string
  /** Underwater nodes (see `CaveGrid.dive`). */
  dive?: string
  /** Ropes and ziplines. */
  links?: Link[]
}

/**
 * `blocked`: grid nodes to route around (found blocked in the game). Routes go as far as
 * they can: to the nearest walkable spot when the goal can't be reached.
 */
export type CaveRequest = { id: number; baseUrl: string; cave: CaveInfo; from: Vec3; to: Vec3; blocked: number[] }
export type CaveResponse = { id: number; route: GridRoute | null; error?: string }

const searches = new Map<string, Promise<GridSearch>>()

async function load(baseUrl: string, cave: CaveInfo): Promise<GridSearch> {
  const key = `${baseUrl}/${cave.file}`
  let search = searches.get(key)
  if (!search) {
    search = (async () => {
      const res = await fetch(key)
      if (!res.ok) throw new Error(`cave grid ${cave.file}: HTTP ${res.status}`)
      // Without its water files a grid still routes, just not by jumping into water or
      // swimming up and down.
      const bits = (file?: string) =>
        file ? fetch(`${baseUrl}/${file}`).then((r) => (r.ok ? r.arrayBuffer().then((b) => new Uint8Array(b)) : undefined), () => undefined) : undefined
      const [floors, water, dive] = await Promise.all([res.arrayBuffer(), bits(cave.water), bits(cave.dive)])
      const grid: CaveGrid = { ...cave, floors: new Int16Array(floors), water, dive }
      return new GridSearch(grid)
    })()
    searches.set(key, search)
    search.catch(() => searches.delete(key))
  }
  return search
}

self.onmessage = async (e: MessageEvent<CaveRequest>) => {
  const { id, baseUrl, cave, from, to, blocked } = e.data
  try {
    const search = await load(baseUrl, cave)
    const response: CaveResponse = { id, route: search.route(from, to, new Set(blocked), true) }
    self.postMessage(response)
  } catch (err) {
    const response: CaveResponse = { id, route: null, error: String(err) }
    self.postMessage(response)
  }
}
