// Pin badge images: a colored disc with a white glyph (game icons are white on
// transparent), rendered once per category/icon and registered with MapLibre.

import type { Map as MapLibreMap } from 'maplibre-gl'
import { iconUrl } from '../lib/game'

const SIZE = 44 // logical px; drawn at 2x for crisp HiDPI rendering
const RATIO = 2

const imageCache = new Map<string, Promise<HTMLImageElement | null>>()

function loadImage(url: string): Promise<HTMLImageElement | null> {
  let p = imageCache.get(url)
  if (!p) {
    p = new Promise((resolve) => {
      const img = new Image()
      img.crossOrigin = 'anonymous'
      img.onload = () => resolve(img)
      img.onerror = () => resolve(null)
      img.src = url
    })
    imageCache.set(url, p)
  }
  return p
}

/** Glyph color with enough contrast against the badge color. */
export function glyphColor(background: string): string {
  const m = /^#?([0-9a-f]{6})$/i.exec(background.trim())
  if (!m) return '#ffffff'
  const n = parseInt(m[1], 16)
  const lin = (c: number) => {
    const v = c / 255
    return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4
  }
  const luminance = 0.2126 * lin(n >> 16) + 0.7152 * lin((n >> 8) & 255) + 0.0722 * lin(n & 255)
  return luminance > 0.45 ? '#1d232a' : '#ffffff'
}

/** Draws a white-on-transparent glyph image recolored to `color`. */
function drawTinted(ctx: CanvasRenderingContext2D, img: HTMLImageElement, x: number, y: number, w: number, h: number, color: string) {
  const off = document.createElement('canvas')
  off.width = Math.ceil(w)
  off.height = Math.ceil(h)
  const o = off.getContext('2d')!
  o.drawImage(img, 0, 0, w, h)
  o.globalCompositeOperation = 'source-in'
  o.fillStyle = color
  o.fillRect(0, 0, w, h)
  ctx.drawImage(off, x, y)
}

function drawPinGlyph(ctx: CanvasRenderingContext2D, s: number, color: string) {
  // Simple map-pin glyph for `builtin:pin`.
  ctx.fillStyle = color
  ctx.beginPath()
  ctx.arc(s / 2, s * 0.42, s * 0.16, 0, Math.PI * 2)
  ctx.fill()
  ctx.beginPath()
  ctx.moveTo(s * 0.36, s * 0.5)
  ctx.lineTo(s / 2, s * 0.74)
  ctx.lineTo(s * 0.64, s * 0.5)
  ctx.fill()
}

/** Renders a badge. `iconRef` may point to a full-color user image, drawn clipped to the disc. */
export async function renderBadge(gameId: string, color: string, iconRef: string): Promise<ImageData> {
  const px = SIZE * RATIO
  const canvas = document.createElement('canvas')
  canvas.width = canvas.height = px
  const ctx = canvas.getContext('2d')!
  const r = px / 2 - 3 * RATIO

  ctx.shadowColor = 'rgba(0,0,0,0.45)'
  ctx.shadowBlur = 4 * RATIO
  ctx.shadowOffsetY = 1 * RATIO
  ctx.fillStyle = color
  ctx.beginPath()
  ctx.arc(px / 2, px / 2, r, 0, Math.PI * 2)
  ctx.fill()
  ctx.shadowColor = 'transparent'
  ctx.lineWidth = 2 * RATIO
  ctx.strokeStyle = 'rgba(255,255,255,0.9)'
  ctx.stroke()

  const url = iconUrl(gameId, iconRef)
  const img = url ? await loadImage(url) : null
  if (img) {
    const user = iconRef.startsWith('user:')
    ctx.save()
    ctx.beginPath()
    ctx.arc(px / 2, px / 2, r - 1.5 * RATIO, 0, Math.PI * 2)
    ctx.clip()
    const inset = user ? 0 : r * 0.45
    const box = (r - inset) * 2
    // Keep aspect ratio inside the box.
    const scale = Math.min(box / img.width, box / img.height)
    const w = img.width * scale
    const h = img.height * scale
    if (user) ctx.drawImage(img, (px - w) / 2, (px - h) / 2, w, h)
    else drawTinted(ctx, img, (px - w) / 2, (px - h) / 2, w, h, glyphColor(color))
    ctx.restore()
  } else {
    drawPinGlyph(ctx, px, glyphColor(color))
  }
  return ctx.getImageData(0, 0, px, px)
}

/** Ensures a badge image exists on the map under `key`. */
export async function ensureBadge(map: MapLibreMap, key: string, gameId: string, color: string, iconRef: string) {
  if (map.hasImage(key)) return
  const data = await renderBadge(gameId, color, iconRef)
  if (!map.hasImage(key)) map.addImage(key, data, { pixelRatio: RATIO })
}

export function badgeKey(color: string, iconRef: string): string {
  return `badge:${color}:${iconRef}`
}
