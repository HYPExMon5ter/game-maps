// How the fog of war looks: a bank of cloud over the map, thinning into wisps at the edge
// of what's been explored. Pure pixels, the same every time; `map/MapView` puts them
// together on a canvas.

/** Dark gaps and bright tops of the clouds. */
const SHADOW: [number, number, number] = [62, 74, 90]
const LIGHT: [number, number, number] = [168, 178, 190]
/** How far in from the edge the clouds fade out, as a share of the map. */
const EDGE_FADE = 0.12
/** Around the map, so the clouds don't stop at a hard edge. */
export const CLOUD_BACKGROUND = `rgb(${SHADOW.join(',')})`

/** A repeatable random value in 0..1 for a grid point. */
function hash(x: number, y: number): number {
  let h = Math.imul(x, 374761393) + Math.imul(y, 668265263)
  h = Math.imul(h ^ (h >>> 13), 1274126177)
  return ((h ^ (h >>> 16)) >>> 0) / 0xffffffff
}

const smooth = (t: number) => t * t * (3 - 2 * t)

function valueNoise(x: number, y: number): number {
  const ix = Math.floor(x)
  const iy = Math.floor(y)
  const fx = smooth(x - ix)
  const fy = smooth(y - iy)
  const top = hash(ix, iy) + (hash(ix + 1, iy) - hash(ix, iy)) * fx
  const bottom = hash(ix, iy + 1) + (hash(ix + 1, iy + 1) - hash(ix, iy + 1)) * fx
  return top + (bottom - top) * fy
}

/** Layered noise, 0..1: big shapes with finer detail on top. */
export function fractalNoise(x: number, y: number, octaves: number): number {
  let sum = 0
  let total = 0
  let amplitude = 1
  for (let i = 0; i < octaves; i++) {
    sum += valueNoise(x, y) * amplitude
    total += amplitude
    amplitude *= 0.5
    x = x * 2 + 17
    y = y * 2 + 31
  }
  return sum / total
}

/** Opaque cloud as RGBA pixels, so nothing under it shows. `size`: pixels per cloud. */
export function cloudPixels(width: number, height: number, size: number): Uint8ClampedArray<ArrayBuffer> {
  const out = new Uint8ClampedArray(new ArrayBuffer(width * height * 4))
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      // Warping by more noise makes billows instead of even blobs.
      const warp = fractalNoise(x / size + 7, y / size + 3, 3) * 0.55
      const n = fractalNoise(x / size + warp, y / size - warp, 6)
      // Toward the shadow color at the edges, to meet what's around the map.
      const edge = Math.min(1, Math.min(x, y, width - 1 - x, height - 1 - y) / (Math.min(width, height) * EDGE_FADE))
      const t = smooth(Math.min(1, Math.max(0, (n - 0.28) / 0.44))) * smooth(edge)
      const o = (y * width + x) * 4
      for (let c = 0; c < 3; c++) out[o + c] = SHADOW[c] + (LIGHT[c] - SHADOW[c]) * t
      out[o + 3] = 255
    }
  }
  return out
}

/**
 * Turns a blurred fog mask (RGBA, alpha 255 = fog) into a ragged edge in place: noise moves
 * where the fog ends, so it breaks up into wisps. `size`: pixels per wisp. Solid fog and
 * clear ground stay as they are.
 */
export function wispyEdge(mask: Uint8ClampedArray, width: number, height: number, size: number) {
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const i = (y * width + x) * 4 + 3
      const a = mask[i] / 255
      if (a === 0 || a === 1) continue
      const shifted = a + (fractalNoise(x / size, y / size, 4) - 0.5) * 0.9
      mask[i] = smooth(Math.min(1, Math.max(0, (shifted - 0.15) / 0.7))) * 255
    }
  }
}
