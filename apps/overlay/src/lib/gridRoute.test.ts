import { describe, expect, it } from 'vitest'
import { GridSearch, NONE, walkableRoute, type CaveGrid, type LegCheck } from './gridRoute'

/** A grid from rows of characters (south row first): `.` floor at 0 m, digits floor at that many meters, `#` rock. */
function grid(rows: string[], layers: string[][] = []): CaveGrid {
  const height = rows.length
  const width = rows[0].length
  const all = [rows, ...layers]
  const floors = new Int16Array(all.length * width * height).fill(NONE)
  all.forEach((layer, l) =>
    layer.forEach((row, r) =>
      [...row].forEach((ch, c) => {
        if (ch === '#' || ch === ' ') return
        floors[l * width * height + r * width + c] = ch === '.' ? 0 : Number(ch) * 10
      }),
    ),
  )
  return { area: 1, originX: 0, originZ: 0, cell: 1, width, height, layers: all.length, floors }
}

const at = (x: number, z: number, y = 0) => ({ x: x + 0.5, y, z: z + 0.5 })

describe('GridSearch', () => {
  it('walks around walls', () => {
    const g = grid(['.....', '####.', '.....'])
    const route = new GridSearch(g).route(at(0, 0), at(0, 2))
    expect(route).not.toBeNull()
    // Has to pass the gap at the east end of the wall.
    expect(route!.corners.some((p) => p.x > 4 && p.z > 1 && p.z < 2)).toBe(true)
  })

  it('climbs small steps but not cliffs', () => {
    expect(new GridSearch(grid(['.....', '#####', '..9..'])).route(at(0, 0), at(4, 2))).toBeNull()
    // 1 m per meter is walkable; 2 m per meter isn't (going up), but dropping down is.
    expect(new GridSearch(grid(['01234'])).route(at(0, 0), at(4, 0, 4))).not.toBeNull()
    const steep = grid(['02468'])
    expect(new GridSearch(steep).route(at(0, 0), at(4, 0, 8))).toBeNull()
    expect(new GridSearch(steep).route(at(4, 0, 8), at(0, 0))).not.toBeNull()
  })

  it('uses floors on other layers', () => {
    // The first layer is blocked in the middle; a second layer has floor there.
    expect(new GridSearch(grid(['..#..'])).route(at(0, 0), at(4, 0))).toBeNull()
    expect(new GridSearch(grid(['..#..'], [['  .  ']])).route(at(0, 0), at(4, 0))).not.toBeNull()
  })

  it('returns only the corners of straight runs', () => {
    const route = new GridSearch(grid(['......'])).route(at(0, 0), at(5, 0))
    expect(route!.corners).toHaveLength(2)
    expect(route!.points).toHaveLength(6)
  })
})

describe('walkableRoute', () => {
  // Two ways from the west end to the east end: straight along the middle, or around.
  const g = grid(['.......', '.......', '.......'])
  const search = new GridSearch(g)
  const find = (blocked: ReadonlySet<number>) => Promise.resolve(search.route(at(0, 1), at(6, 1), blocked))
  /** Blocks legs that enter the cell at (3, 1): a rock in the middle. */
  const rockCheck: LegCheck = (points) =>
    Promise.resolve({
      blocked: points.flatMap((p, i) => (i > 0 && Math.floor(p.x) === 3 && Math.floor(p.z) === 1 ? [i - 1] : [])),
      checked: points.length - 1,
    })

  it('routes around spots the game reports blocked', async () => {
    const blocked = new Map<number, number>()
    const route = await walkableRoute(find, rockCheck, blocked, () => 7)
    expect(route).not.toBeNull()
    expect(route!.points.some((p) => Math.floor(p.x) === 3 && Math.floor(p.z) === 1)).toBe(false)
    expect([...blocked.values()]).toEqual([7])
  })

  it('checks long routes in pieces', async () => {
    const calls: number[] = []
    const oneLegAtATime: LegCheck = (points) => {
      calls.push(points.length)
      return Promise.resolve({ blocked: [], checked: 1 })
    }
    const route = await walkableRoute(find, oneLegAtATime, new Map())
    expect(calls).toHaveLength(route!.points.length - 1)
  })

  it('keeps the unchecked route when the game cannot check it', async () => {
    const route = await walkableRoute(find, () => Promise.reject(new Error('not connected')), new Map())
    expect(route!.points).toHaveLength(7)
  })

  it('never blocks the destination', async () => {
    const all: LegCheck = (points) => Promise.resolve({ blocked: points.slice(1).map((_, i) => i), checked: points.length - 1 })
    const blocked = new Map<number, number>()
    await walkableRoute(find, all, blocked)
    expect(blocked.has(g.width * 1 + 6)).toBe(false)
  })

  it('is null only when there is no way around the blocks', async () => {
    // A corridor one cell wide: nothing to route around a rock with.
    const corridor = new GridSearch(grid(['.......']))
    const route = await walkableRoute((b) => Promise.resolve(corridor.route(at(0, 0), at(6, 0), b)), (points) =>
      Promise.resolve({ blocked: points.flatMap((p, i) => (i > 0 && Math.floor(p.x) === 3 ? [i - 1] : [])), checked: points.length - 1 }),
    new Map())
    expect(route).toBeNull()
  })
})
