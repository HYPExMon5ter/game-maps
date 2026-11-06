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
/** Largest side of the mask: blurring and breaking up its edge is most of a redraw. */
const MAX_MASK_SIDE = 1024
/** Largest side of the finished image, which is redrawn and uploaded while exploring. */
const MAX_IMAGE_SIDE = 2048

/** Image pixels per cell for a fog: fewer on huge maps, where cells are large anyway. */
export function imageScale(fog: Pick<Fog, 'width' | 'height'>): number {
  return Math.min(SCALE, MAX_IMAGE_SIDE / Math.max(fog.width, fog.height))
}

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

/** Mask pixels per cell: fewer than one on huge maps, where a cell's edge can't be seen. */
export function maskScale(fog: Pick<Fog, 'width' | 'height'>): number {
  return Math.min(1, MAX_MASK_SIDE / Math.max(fog.width, fog.height))
}

/** Where fog stays, with ragged edges. */
function fogMask(fog: Fog): HTMLCanvasElement {
  const scale = maskScale(fog)
  const width = Math.round(fog.width * scale)
  const height = Math.round(fog.height * scale)
  const blur = EDGE_BLUR * scale
  const pad = Math.ceil(blur * 3)
  // Past the map's edge counts as fog, so blurring doesn't thin it out along the rim.
  const [padded, paddedCtx] = canvasOf(width + pad * 2, height + pad * 2)
  paddedCtx.fillRect(0, 0, padded.width, padded.height)
  paddedCtx.clearRect(pad, pad, width, height)
  const [cells, cellsCtx] = canvasOf(fog.width, fog.height)
  cellsCtx.putImageData(new ImageData(fogPixels(fog, [0, 0, 0]), fog.width, fog.height), 0, 0)
  paddedCtx.drawImage(cells, pad, pad, width, height)

  const [mask, maskCtx] = canvasOf(width, height)
  maskCtx.filter = `blur(${blur}px)`
  maskCtx.drawImage(padded, -pad, -pad)
  maskCtx.filter = 'none'
  const pixels = maskCtx.getImageData(0, 0, width, height)
  wispyEdge(pixels.data, width, height, WISP_CELLS * scale)
  maskCtx.putImageData(pixels, 0, 0)
  return mask
}

/** The fog over the whole map, north up. */
export function drawFog(fog: Fog): HTMLCanvasElement {
  const scale = imageScale(fog)
  const [canvas, ctx] = canvasOf(Math.round(fog.width * scale), Math.round(fog.height * scale))
  ctx.imageSmoothingQuality = 'high'
  ctx.drawImage(cloudCanvas(fog), 0, 0, canvas.width, canvas.height)
  ctx.globalCompositeOperation = 'destination-in'
  ctx.drawImage(fogMask(fog), 0, 0, canvas.width, canvas.height)
  return canvas
}
