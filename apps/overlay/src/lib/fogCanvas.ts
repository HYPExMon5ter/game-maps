// Draws the fog of war for the map: clouds, with what's been explored cut out of them.

import { cloudPixels, wispyEdge } from './clouds'
import { fogPixels, type Fog } from './fog'

/** Image pixels per fog cell. */
const SCALE = 2
/** How far the fog's edge is blurred before it's broken up, in cells. */
const EDGE_BLUR = 6
/** Sizes in cells: one bank of cloud, and one wisp at its edge. */
const CLOUD_CELLS = 70
const WISP_CELLS = 14
/** Largest side of the cloud texture: on huge maps it's stretched instead of redone. */
const MAX_CLOUD_SIDE = 1024

function canvasOf(width: number, height: number): [HTMLCanvasElement, CanvasRenderingContext2D] {
  const canvas = document.createElement('canvas')
  canvas.width = width
  canvas.height = height
  const ctx = canvas.getContext('2d', { willReadFrequently: true })
  if (!ctx) throw new Error('no 2d canvas')
  return [canvas, ctx]
}

/** The clouds never change, and exploring redraws the fog often. */
let clouds: { key: string; canvas: HTMLCanvasElement } | null = null

function cloudCanvas(fog: Fog): HTMLCanvasElement {
  const key = `${fog.width}x${fog.height}`
  if (clouds?.key === key) return clouds.canvas
  const shrink = Math.min(1, MAX_CLOUD_SIDE / (Math.max(fog.width, fog.height) * SCALE))
  const width = Math.round(fog.width * SCALE * shrink)
  const height = Math.round(fog.height * SCALE * shrink)
  const [canvas, ctx] = canvasOf(width, height)
  ctx.putImageData(new ImageData(cloudPixels(width, height, CLOUD_CELLS * SCALE * shrink), width, height), 0, 0)
  clouds = { key, canvas }
  return canvas
}

/** Where fog stays, with ragged edges, one pixel per cell. */
function fogMask(fog: Fog): HTMLCanvasElement {
  const { width, height } = fog
  const pad = EDGE_BLUR * 3
  // Past the map's edge counts as fog, so blurring doesn't thin it out along the rim.
  const [padded, paddedCtx] = canvasOf(width + pad * 2, height + pad * 2)
  paddedCtx.fillRect(0, 0, padded.width, padded.height)
  paddedCtx.clearRect(pad, pad, width, height)
  const [cells, cellsCtx] = canvasOf(width, height)
  cellsCtx.putImageData(new ImageData(fogPixels(fog, [0, 0, 0]), width, height), 0, 0)
  paddedCtx.drawImage(cells, pad, pad)

  const [mask, maskCtx] = canvasOf(width, height)
  maskCtx.filter = `blur(${EDGE_BLUR}px)`
  maskCtx.drawImage(padded, -pad, -pad)
  maskCtx.filter = 'none'
  const pixels = maskCtx.getImageData(0, 0, width, height)
  wispyEdge(pixels.data, width, height, WISP_CELLS)
  maskCtx.putImageData(pixels, 0, 0)
  return mask
}

/** The fog over the whole map, north up. */
export function drawFog(fog: Fog): HTMLCanvasElement {
  const [canvas, ctx] = canvasOf(fog.width * SCALE, fog.height * SCALE)
  ctx.imageSmoothingQuality = 'high'
  ctx.drawImage(cloudCanvas(fog), 0, 0, canvas.width, canvas.height)
  ctx.globalCompositeOperation = 'destination-in'
  ctx.drawImage(fogMask(fog), 0, 0, canvas.width, canvas.height)
  return canvas
}
