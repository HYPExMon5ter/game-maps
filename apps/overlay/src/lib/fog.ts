// Fog of war: which parts of a map the player has been near, as a bitmap over the map's
// world rectangle (one bit per cell). Pure data; see `state/fog` for recording and
// saving, `map/MapView` for drawing.

import type { Projection } from './projection'

/** Cells are this many meters wide, or wider for huge maps (see `MAX_SIDE`). */
const CELL_METERS = 8
/** Largest bitmap side in cells (2048² bits = 512 KB). */
const MAX_SIDE = 2048

export interface Fog {
  /** World units per cell. */
  cell: number
  minEast: number
  minNorth: number
  width: number
  height: number
  /** Bit per cell, row-major from (minEast, minNorth). */
  bits: Uint8Array
}

/** Stored form (base64 bits). */
export interface SavedFog {
  cell: number
  minEast: number
  minNorth: number
  width: number
  height: number
  bits: string
}

export function emptyFog(p: Projection, unitsPerMeter: number): Fog {
  const spanEast = p.maxEast - p.minEast
  const spanNorth = p.maxNorth - p.minNorth
  const cell = Math.max(CELL_METERS * unitsPerMeter, spanEast / MAX_SIDE, spanNorth / MAX_SIDE)
  const width = Math.ceil(spanEast / cell)
  const height = Math.ceil(spanNorth / cell)
  return { cell, minEast: p.minEast, minNorth: p.minNorth, width, height, bits: new Uint8Array(Math.ceil((width * height) / 8)) }
}

export function isExplored(fog: Fog, col: number, row: number): boolean {
  if (col < 0 || row < 0 || col >= fog.width || row >= fog.height) return false
  const i = row * fog.width + col
  return (fog.bits[i >> 3] & (1 << (i & 7))) !== 0
}

/**
 * Reveals the cells within `radius` world units of (east, north). Returns whether any
 * cell was newly revealed.
 */
export function reveal(fog: Fog, east: number, north: number, radius: number): boolean {
  const cx = (east - fog.minEast) / fog.cell
  const cy = (north - fog.minNorth) / fog.cell
  const r = radius / fog.cell
  let changed = false
  for (let row = Math.max(0, Math.floor(cy - r)); row <= Math.min(fog.height - 1, Math.ceil(cy + r)); row++) {
    for (let col = Math.max(0, Math.floor(cx - r)); col <= Math.min(fog.width - 1, Math.ceil(cx + r)); col++) {
      const dx = col + 0.5 - cx
      const dy = row + 0.5 - cy
      if (dx * dx + dy * dy > r * r) continue
      const i = row * fog.width + col
      const mask = 1 << (i & 7)
      if (!(fog.bits[i >> 3] & mask)) {
        fog.bits[i >> 3] |= mask
        changed = true
      }
    }
  }
  return changed
}

/** Whether the world point (east, north) is in an explored cell. Outside the map counts as unexplored. */
export function exploredAt(fog: Fog, east: number, north: number): boolean {
  return isExplored(fog, Math.floor((east - fog.minEast) / fog.cell), Math.floor((north - fog.minNorth) / fog.cell))
}

/** Fraction of the map revealed, 0..1. */
export function exploredFraction(fog: Fog): number {
  let n = 0
  for (const byte of fog.bits) {
    let b = byte
    while (b) {
      b &= b - 1
      n++
    }
  }
  return n / (fog.width * fog.height)
}

export function saveFog(fog: Fog): SavedFog {
  let binary = ''
  for (let i = 0; i < fog.bits.length; i += 0x8000) binary += String.fromCharCode(...fog.bits.subarray(i, i + 0x8000))
  return { cell: fog.cell, minEast: fog.minEast, minNorth: fog.minNorth, width: fog.width, height: fog.height, bits: btoa(binary) }
}

/** Adds what `other` has explored to `fog`; true if that revealed anything. */
export function mergeFog(fog: Fog, other: Fog): boolean {
  if (other.bits.length !== fog.bits.length) return false
  let changed = false
  for (let i = 0; i < fog.bits.length; i++) {
    const merged = fog.bits[i] | other.bits[i]
    if (merged !== fog.bits[i]) {
      fog.bits[i] = merged
      changed = true
    }
  }
  return changed
}

/** A saved fog, if it still fits `blank` (the map's extent may change with a map update). */
export function loadFog(saved: SavedFog | null | undefined, blank: Fog): Fog {
  if (!saved || saved.cell !== blank.cell || saved.minEast !== blank.minEast || saved.minNorth !== blank.minNorth || saved.width !== blank.width || saved.height !== blank.height) {
    return blank
  }
  const binary = atob(saved.bits)
  if (binary.length !== blank.bits.length) return blank
  const bits = new Uint8Array(binary.length)
  for (let i = 0; i < binary.length; i++) bits[i] = binary.charCodeAt(i)
  return { ...blank, bits }
}

/**
 * The fog as RGBA pixels, north up (row 0 is the map's north edge): unexplored cells
 * opaque, explored ones clear.
 */
export function fogPixels(fog: Fog, rgb: [number, number, number]): Uint8ClampedArray<ArrayBuffer> {
  const out = new Uint8ClampedArray(new ArrayBuffer(fog.width * fog.height * 4))
  for (let row = 0; row < fog.height; row++) {
    const y = fog.height - 1 - row
    for (let col = 0; col < fog.width; col++) {
      const o = (y * fog.width + col) * 4
      out[o] = rgb[0]
      out[o + 1] = rgb[1]
      out[o + 2] = rgb[2]
      out[o + 3] = isExplored(fog, col, row) ? 0 : 255
    }
  }
  return out
}
