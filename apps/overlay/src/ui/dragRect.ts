// Dragging a box on the screen (the floating map, and the boxes of edit mode): while the
// pointer moves only the element's own style changes, so nothing else has to keep up.

import type { PointerEvent as ReactPointerEvent } from 'react'
import type { Rect } from '../lib/layout'

export function applyRect(el: HTMLElement | null, r: Rect) {
  if (!el) return
  el.style.left = `${r.left}px`
  el.style.top = `${r.top}px`
  el.style.width = `${r.width}px`
  el.style.height = `${r.height}px`
}

/** Follows a pointer from `e` until it's released, reporting how far it has moved. */
export function trackDrag(e: ReactPointerEvent, onMove: (dx: number, dy: number) => void, onEnd: () => void) {
  e.preventDefault()
  const el = e.currentTarget as HTMLElement
  const { clientX: x, clientY: y, pointerId } = e
  el.setPointerCapture(pointerId)
  const move = (ev: PointerEvent) => onMove(ev.clientX - x, ev.clientY - y)
  const end = () => {
    el.removeEventListener('pointermove', move)
    el.removeEventListener('pointerup', end)
    el.removeEventListener('pointercancel', end)
    onEnd()
  }
  el.addEventListener('pointermove', move)
  el.addEventListener('pointerup', end)
  el.addEventListener('pointercancel', end)
}
