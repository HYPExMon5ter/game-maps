import { describe, expect, it } from 'vitest'
import type { Category, Pin } from './game'
import { breadcrumbs, nextStep, routeDestination, trackPath } from './route'

const v = (x: number, z: number) => ({ x, y: 0, z })

describe('routeDestination', () => {
  const categories: Category[] = [{ id: 'cave', name: 'Caves', color: '#000', icon: 'builtin:pin', visible: true, entrance: true }]
  const pin = (id: string, x: number, extra: Partial<Pin> = {}): Pin => ({ id, category: 'tool', name: id, position: v(x, 0), ...extra })
  const shovel = pin('Shovel', 50, { layer: 'caves' })
  const caveC = pin('Cave C', 100, { category: 'cave', description: 'Contains: Shovel, Slingshot' })
  const caveCExit = pin('Cave C exit', 55, { category: 'cave', description: 'Contains: Shovel, Slingshot' })
  const caveD = pin('Cave D', 51, { category: 'cave', description: 'Contains: Rope' })
  const pins = [shovel, caveC, caveCExit, caveD]

  it('routes to the entrance that lists an underground target', () => {
    expect(routeDestination(shovel, false, pins, categories)).toEqual(caveC.position)
  })

  it('routes straight to the target once underground, or for surface targets', () => {
    expect(routeDestination(shovel, true, pins, categories)).toEqual(shovel.position)
    expect(routeDestination(caveD, false, pins, categories)).toEqual(caveD.position)
  })
})

describe('breadcrumbs', () => {
  // An L-shaped route: 10 east, then 10 north (20 long).
  const route = [v(0, 0), v(10, 0), v(10, 10)]
  const positions = (crumbs: ReturnType<typeof breadcrumbs>) => crumbs.map((c) => c.position)

  it('follows the route ahead of the player, spaced back from its end', () => {
    const crumbs = breadcrumbs(route, v(4, 1), 12, 4)
    expect(positions(crumbs)).toEqual([v(8, 0), v(10, 2), v(10, 6)])
    expect(crumbs.map((c) => c.key)).toEqual([3, 2, 1])
    expect(crumbs[0].direction).toEqual({ x: 1, y: 0, z: 0 })
    expect(crumbs[1].direction).toEqual({ x: 0, y: 0, z: 1 })
  })

  it('stays put as the player walks', () => {
    const before = breadcrumbs(route, v(4, 0), 20, 4)
    const after = breadcrumbs([v(5, 0), v(10, 0), v(10, 10)], v(5, 0), 20, 4)
    expect(positions(after)).toEqual(positions(before).filter((p) => p.x > 5 || p.z > 0))
  })

  it('stops after the look-ahead distance', () => {
    expect(positions(breadcrumbs(route, v(0, 0), 7, 2))).toEqual([v(2, 0), v(4, 0), v(6, 0)])
  })

  it('needs at least one segment', () => {
    expect(breadcrumbs([v(0, 0)], v(0, 0), 10, 1)).toEqual([])
  })
})

describe('nextStep', () => {
  // Walk 10, climb a 6 m rope, walk 5, then two legs of diving.
  const corners = [v(0, 0), v(10, 0), { x: 10, y: 6, z: 0 }, { x: 15, y: 6, z: 0 }, { x: 20, y: 6, z: 0 }, { x: 25, y: 6, z: 0 }]
  const route = { corners, complete: true, moves: ['walk', 'rope', 'walk', 'dive', 'dive'] as const }

  it('finds the first climb, ride or swim and how far it is', () => {
    const path = trackPath(v(0, 0), corners[5], { ...route, moves: [...route.moves] })
    expect(nextStep(path)).toEqual({ move: 'rope', at: 10, length: 6, rise: 6 })
  })

  it('takes a run of legs as one', () => {
    const path = trackPath(v(10, 0), corners[5], { ...route, corners: corners.slice(1), moves: route.moves.slice(1) })
    const rest = { ...path, moves: path.moves.map((m) => (m === 'rope' ? 'walk' : m)) }
    expect(nextStep(rest)).toEqual({ move: 'dive', at: 6 + 5, length: 10, rise: 0 })
  })

  it('has nothing to say about walked routes or the straight line past a route', () => {
    expect(nextStep(trackPath(v(0, 0), v(30, 0), { corners: [v(0, 0), v(10, 0)], complete: true }))).toBeNull()
    expect(nextStep(trackPath(v(0, 0), v(30, 0), null))).toBeNull()
  })
})
