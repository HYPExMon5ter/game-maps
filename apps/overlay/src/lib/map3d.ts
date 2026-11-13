// A game's 3D map data (`games/<id>/3d/index.json`, written by tools/mapgen/mesh3d.py):
// terrain elevation tiles for tilting the surface map, and meshes (caves, bunkers) for
// the underground 3D view. Every part is optional; a map pack without it has no 3D.

import { ASSET_BASE } from './game'
import type { Projection } from './projection'
import type { Vec3 } from './protocol'

export interface TerrainInfo {
  /** Tile URL template, relative to the game folder. */
  url: string
  tileSize: number
  minNativeZoom: number
  maxNativeZoom: number
  minHeight: number
  maxHeight: number
}

export interface MeshInfo {
  id: string
  name: string
  /** Mesh file, relative to the game folder. */
  file: string
  /** World bounds (game axes). */
  min: [number, number, number]
  max: [number, number, number]
  triangles: number
  /** The area's water surfaces (same mesh format), if it has any. */
  water?: string
}

export interface Map3D {
  terrain?: TerrainInfo
  meshes: MeshInfo[]
}

const indexes = new Map<string, Promise<Record<string, Map3D>>>()

export function gameAssetUrl(gameId: string, path: string): string {
  return `${ASSET_BASE}/games/${encodeURIComponent(gameId)}/${path}`
}

/** The 3D data of a game's map, or null when its map pack has none. */
export async function load3d(gameId: string, mapId: string): Promise<Map3D | null> {
  let index = indexes.get(gameId)
  if (!index) {
    index = fetch(gameAssetUrl(gameId, '3d/index.json'))
      .then((r) => (r.ok ? (r.json() as Promise<{ version: number; maps?: Record<string, Partial<Map3D>> }>) : null))
      .then((d) => {
        const out: Record<string, Map3D> = {}
        if (d?.version !== 1) return out
        for (const [id, m] of Object.entries(d.maps ?? {})) out[id] = { terrain: m.terrain, meshes: m.meshes ?? [] }
        return out
      })
      .catch(() => ({}))
    indexes.set(gameId, index)
  }
  const map = (await index)[mapId]
  return map && (map.terrain || map.meshes.length) ? map : null
}

/** Forgets loaded indexes (a new map pack was installed). */
export function reset3d() {
  indexes.clear()
}

/** A decoded mesh file: positions quantized over `min`..`max`, as stored. */
export interface MeshData {
  /** 3 per vertex, 0..65535 across the bounds. */
  positions: Uint16Array
  /** Smooth unit normals, 2 per vertex (octahedral, -127..127), in the game's axes. */
  normals: Int8Array
  indices: Uint16Array | Uint32Array
  min: [number, number, number]
  max: [number, number, number]
}

const MAGIC = 0x4433_4d47 // "GM3D" little-endian
const VERSION = 2
const HEADER_BYTES = 40

/** Parses the `.bin` mesh format (see tools/mapgen/mesh3d.py). No copies are made. */
export function decodeMesh(buffer: ArrayBuffer): MeshData {
  const view = new DataView(buffer)
  if (buffer.byteLength < HEADER_BYTES || view.getUint32(0, true) !== MAGIC || view.getUint32(4, true) !== VERSION) {
    throw new Error(`not a GM3D v${VERSION} mesh`)
  }
  const vertices = view.getUint32(8, true)
  const triangles = view.getUint32(12, true)
  const f = (o: number) => view.getFloat32(o, true)
  const min: [number, number, number] = [f(16), f(20), f(24)]
  const max: [number, number, number] = [f(28), f(32), f(36)]
  const pad = (n: number) => Math.ceil(n / 4) * 4
  const positions = new Uint16Array(buffer, HEADER_BYTES, vertices * 3)
  const normalOffset = HEADER_BYTES + pad(vertices * 6)
  const normals = new Int8Array(buffer, normalOffset, vertices * 2)
  const indexOffset = normalOffset + pad(vertices * 2)
  const indices = vertices <= 65536 ? new Uint16Array(buffer, indexOffset, triangles * 3) : new Uint32Array(buffer, indexOffset, triangles * 3)
  if (indexOffset + indices.byteLength > buffer.byteLength) throw new Error('truncated mesh')
  return { positions, normals, indices, min, max }
}

export async function loadMesh(gameId: string, file: string, signal?: AbortSignal): Promise<MeshData> {
  const r = await fetch(gameAssetUrl(gameId, file), { signal })
  if (!r.ok) throw new Error(`${file}: HTTP ${r.status}`)
  return decodeMesh(await r.arrayBuffer())
}

/** Whether a world position is inside a mesh's bounds (grown by `margin`). */
export function meshContains(m: MeshInfo, p: Vec3, margin = 4): boolean {
  return (
    p.x >= m.min[0] - margin &&
    p.x <= m.max[0] + margin &&
    p.y >= m.min[1] - margin &&
    p.y <= m.max[1] + margin &&
    p.z >= m.min[2] - margin &&
    p.z <= m.max[2] + margin
  )
}

/**
 * The mesh a position is in, or else the nearest one. Where bounds overlap (a bunker
 * inside a big cave's box) the smaller one wins. `withHeight: false` compares only
 * map positions (a point picked on the 2D map has no height).
 */
export function meshAt(meshes: MeshInfo[], p: Vec3, pr: Projection, withHeight = true): MeshInfo | undefined {
  const axis = { x: 0, y: 1, z: 2 } as const
  const e = axis[pr.east]
  const n = axis[pr.north]
  const u = axis[pr.up]
  const outside = (m: MeshInfo, i: number, v: number) => Math.max(m.min[i] - v, 0, v - m.max[i])
  const gap = (m: MeshInfo) =>
    Math.max(0, Math.hypot(outside(m, e, p[pr.east]), outside(m, n, p[pr.north]), withHeight ? outside(m, u, p[pr.up]) : 0) - CONTAIN_MARGIN)
  const area = (m: MeshInfo) => (m.max[e] - m.min[e]) * (m.max[n] - m.min[n])
  return [...meshes].sort((a, b) => gap(a) - gap(b) || area(a) - area(b))[0]
}

/** Slack around mesh bounds when finding the mesh a position is in (world units). */
const CONTAIN_MARGIN = 4

/** Equator circumference of MapLibre's Web Mercator world, in meters. */
const EARTH_CIRCUMFERENCE = 40_075_016.686

/**
 * Terrain exaggeration that shows the map's elevation at true scale. The map places
 * the whole world square on one Mercator tile, so a world unit spans far more than a
 * meter there; MapLibre scales elevation by meters at the latitude in the middle of
 * the view, so this is exact for a view centered at `centerLat`.
 */
export function trueScaleExaggeration(p: Projection, centerLat: number): number {
  const tileSize = 1 / 2 ** (p.tileZoom ?? 0)
  const perUnit = tileSize / (p.maxEast - p.minEast)
  return perUnit * EARTH_CIRCUMFERENCE * Math.cos((centerLat * Math.PI) / 180)
}
