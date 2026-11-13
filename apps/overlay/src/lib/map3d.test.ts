import { describe, expect, it } from 'vitest'
import { decodeMesh, meshAt, trueScaleExaggeration, type MeshInfo } from './map3d'
import type { Projection } from './projection'

const SOTF: Projection = { east: 'x', north: 'z', up: 'y', minEast: -2000, maxEast: 2000, minNorth: -2000, maxNorth: 2000, tileZoom: 3, tileX: 3, tileY: 3 }

/** A mesh file as tools/mapgen/mesh3d.py writes it. */
function meshFile(vertices: number[][], triangles: number[][], min: number[], max: number[]): ArrayBuffer {
  const big = vertices.length > 65536
  const posBytes = Math.ceil((vertices.length * 6) / 4) * 4
  const normalBytes = Math.ceil((vertices.length * 2) / 4) * 4
  const buf = new ArrayBuffer(40 + posBytes + normalBytes + triangles.length * 3 * (big ? 4 : 2))
  const v = new DataView(buf)
  ;[0x47, 0x4d, 0x33, 0x44].forEach((b, i) => v.setUint8(i, b))
  v.setUint32(4, 2, true)
  v.setUint32(8, vertices.length, true)
  v.setUint32(12, triangles.length, true)
  ;[...min, ...max].forEach((f, i) => v.setFloat32(16 + i * 4, f, true))
  vertices.flat().forEach((q, i) => v.setUint16(40 + i * 2, q, true))
  // Every normal straight up the oct map's z (0, 0).
  const at = 40 + posBytes + normalBytes
  triangles.flat().forEach((t, i) => (big ? v.setUint32(at + i * 4, t, true) : v.setUint16(at + i * 2, t, true)))
  return buf
}

describe('decodeMesh', () => {
  it('reads positions, indices and bounds without copying', () => {
    const buf = meshFile(
      [
        [0, 0, 0],
        [65535, 0, 0],
        [0, 65535, 65535],
      ],
      [[0, 1, 2]],
      [-10, 0, 5],
      [10, 4, 6],
    )
    const m = decodeMesh(buf)
    expect(m.positions.buffer).toBe(buf)
    expect([...m.positions]).toEqual([0, 0, 0, 65535, 0, 0, 0, 65535, 65535])
    expect([...m.indices]).toEqual([0, 1, 2])
    expect(m.normals.length).toBe(6)
    expect(m.min).toEqual([-10, 0, 5])
    expect(m.max).toEqual([10, 4, 6])
  })

  it('rejects other files', () => {
    expect(() => decodeMesh(new ArrayBuffer(64))).toThrow()
    expect(() => decodeMesh(meshFile([[0, 0, 0]], [[0, 0, 0]], [0, 0, 0], [1, 1, 1]).slice(0, 44))).toThrow()
  })
})

const mesh = (id: string, min: [number, number, number], max: [number, number, number]): MeshInfo => ({ id, name: id, file: `${id}.bin`, min, max, triangles: 1 })

describe('meshAt', () => {
  const bigCave = mesh('cave', [0, 0, 0], [1000, 100, 1000])
  const bunker = mesh('bunker', [400, 40, 400], [500, 60, 500])
  const other = mesh('other', [2000, 0, 2000], [2100, 50, 2100])
  const all = [bigCave, bunker, other]

  it('picks the smallest model around a position', () => {
    expect(meshAt(all, { x: 450, y: 50, z: 450 }, SOTF)?.id).toBe('bunker')
    expect(meshAt(all, { x: 100, y: 50, z: 100 }, SOTF)?.id).toBe('cave')
  })

  it('takes height into account only when asked', () => {
    expect(meshAt(all, { x: 450, y: 90, z: 450 }, SOTF)?.id).toBe('cave')
    expect(meshAt(all, { x: 450, y: 0, z: 450 }, SOTF, false)?.id).toBe('bunker')
  })

  it('falls back to the nearest model', () => {
    expect(meshAt(all, { x: 1900, y: 0, z: 1900 }, SOTF, false)?.id).toBe('other')
  })
})

describe('trueScaleExaggeration', () => {
  it('scales meters to the size of a world unit on the map tile', () => {
    // A 4 km world on one zoom-3 tile: each unit spans 1/32000 of the Mercator world.
    expect(trueScaleExaggeration(SOTF, 0)).toBeCloseTo(40_075_016.686 / 32_000, 3)
    expect(trueScaleExaggeration(SOTF, 60)).toBeCloseTo(40_075_016.686 / 64_000, 3)
  })
})
