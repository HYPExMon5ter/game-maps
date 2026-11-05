import { describe, expect, it } from 'vitest'
import type { Projection } from './projection'
import type { LocalPlayer, WorldLayer } from './protocol'
import { extendTrail, trailLines, type TrailPoint } from './trail'

const projection = { east: 'x', north: 'z' } as Projection
const at = (x: number, layer: WorldLayer = 'surface'): LocalPlayer => ({ position: { x, y: 0, z: 0 }, forward: null, layer })
const walk = (...xs: number[]) => xs.reduce<TrailPoint[]>((t, x) => extendTrail(t, at(x), projection, 1), [])

describe('extendTrail', () => {
  it('starts with the first position', () => {
    expect(walk(10)).toEqual([{ position: { x: 10, y: 0, z: 0 }, layer: 'surface', start: true }])
  })

  it('ignores small steps, keeping the same array', () => {
    const trail = walk(0)
    expect(extendTrail(trail, at(4), projection, 1)).toBe(trail)
  })

  it('adds a point every few meters', () => {
    expect(walk(0, 6, 12).map((p) => p.position.x)).toEqual([0, 6, 12])
  })

  it('breaks the line at a teleport', () => {
    expect(walk(0, 6, 500).map((p) => p.start)).toEqual([true, false, true])
  })

  it('breaks the line at a change of layer', () => {
    const trail = extendTrail(walk(0), at(1, 'caves'), projection, 1)
    expect(trail.map((p) => [p.layer, p.start])).toEqual([
      ['surface', true],
      ['caves', true],
    ])
  })

  it('forgets the oldest points', () => {
    let trail: TrailPoint[] = []
    for (let i = 0; i < 5100; i++) trail = extendTrail(trail, at(i * 6), projection, 1)
    expect(trail).toHaveLength(5000)
    expect(trail[0].position.x).toBe(100 * 6)
  })
})

describe('trailLines', () => {
  it('splits at breaks and drops single points', () => {
    const lines = trailLines(walk(0, 6, 500, 506, 1000), 'surface')
    expect(lines.map((l) => l.map((p) => p.x))).toEqual([
      [0, 6],
      [500, 506],
    ])
  })

  it('shows one layer', () => {
    const trail = [...walk(0, 6), { position: { x: 9, y: 0, z: 0 }, layer: 'caves' as const, start: true }, { position: { x: 15, y: 0, z: 0 }, layer: 'caves' as const, start: false }]
    expect(trailLines(trail, 'caves').map((l) => l.map((p) => p.x))).toEqual([[9, 15]])
  })
})
