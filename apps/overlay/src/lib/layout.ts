// Where the floating map and the mini-map sit on the screen. Everything is stored as a
// fraction of the screen, so a layout survives a change of resolution or monitor.

/** A rectangle in screen fractions (0..1). */
export interface FractionRect {
  x: number
  y: number
  width: number
  height: number
}

/** A rectangle in pixels. */
export interface Rect {
  left: number
  top: number
  width: number
  height: number
}

export interface Size {
  width: number
  height: number
}

/** Which edges a resize handle moves. */
export type Handle = 'n' | 's' | 'e' | 'w' | 'ne' | 'nw' | 'se' | 'sw'

export const HANDLES: readonly Handle[] = ['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w']

/** The smallest the floating map may get, in pixels (its panels need the room). */
export const MIN_MAP_PANEL: Size = { width: 720, height: 480 }

const clamp = (v: number, min: number, max: number) => Math.min(max, Math.max(min, v))

/** `rect` moved so it's fully on a screen of `bounds`, and no larger than it. */
export function fitRect(rect: Rect, bounds: Size): Rect {
  const width = Math.min(rect.width, bounds.width)
  const height = Math.min(rect.height, bounds.height)
  return { left: clamp(rect.left, 0, bounds.width - width), top: clamp(rect.top, 0, bounds.height - height), width, height }
}

/** `start` moved by a drag of (`dx`, `dy`), kept on the screen. */
export function moveRect(start: Rect, dx: number, dy: number, bounds: Size): Rect {
  return fitRect({ ...start, left: start.left + dx, top: start.top + dy }, bounds)
}

/**
 * `start` resized by dragging `handle` by (`dx`, `dy`). The opposite edges stay put, and
 * the result keeps to `min` and `max` and stays on the screen.
 */
export function resizeRect(start: Rect, handle: Handle, dx: number, dy: number, min: Size, max: Size, bounds: Size): Rect {
  const right = start.left + start.width
  const bottom = start.top + start.height
  let { left, top, width, height } = start
  if (handle.includes('e')) width = clamp(start.width + dx, min.width, Math.min(max.width, bounds.width - start.left))
  if (handle.includes('s')) height = clamp(start.height + dy, min.height, Math.min(max.height, bounds.height - start.top))
  if (handle.includes('w')) {
    width = clamp(start.width - dx, min.width, Math.min(max.width, right))
    left = right - width
  }
  if (handle.includes('n')) {
    height = clamp(start.height - dy, min.height, Math.min(max.height, bottom))
    top = bottom - height
  }
  return { left, top, width, height }
}

/** `start` at `scale` times its size around its center, kept on the screen. */
export function scaleRect(start: Rect, width: number, height: number, bounds: Size): Rect {
  const cx = start.left + start.width / 2
  const cy = start.top + start.height / 2
  return fitRect({ left: cx - width / 2, top: cy - height / 2, width, height }, bounds)
}

export function toPixels(r: FractionRect, screen: Size): Rect {
  return { left: r.x * screen.width, top: r.y * screen.height, width: r.width * screen.width, height: r.height * screen.height }
}

export function toFractions(r: Rect, screen: Size): FractionRect {
  return { x: r.left / screen.width, y: r.top / screen.height, width: r.width / screen.width, height: r.height / screen.height }
}

/** Where a panel of `size` sits: `anchor` runs 0..1 across the room left inside `margin`. */
export function anchoredRect(anchor: { x: number; y: number }, size: Size, screen: Size, margin: number): Rect {
  const roomX = Math.max(screen.width - size.width - 2 * margin, 0)
  const roomY = Math.max(screen.height - size.height - 2 * margin, 0)
  return { left: margin + anchor.x * roomX, top: margin + anchor.y * roomY, ...size }
}

/** The anchor that puts a panel at `rect` (see `anchoredRect`). */
export function anchorOf(rect: Rect, screen: Size, margin: number): { x: number; y: number } {
  const roomX = screen.width - rect.width - 2 * margin
  const roomY = screen.height - rect.height - 2 * margin
  return {
    x: roomX > 0 ? clamp((rect.left - margin) / roomX, 0, 1) : 0,
    y: roomY > 0 ? clamp((rect.top - margin) / roomY, 0, 1) : 0,
  }
}
