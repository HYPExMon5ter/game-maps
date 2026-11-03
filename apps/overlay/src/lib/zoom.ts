// Keyboard zoom for the full map and the mini-map.

/** Zoom levels per key press (each level doubles the scale). */
export const ZOOM_STEP = 0.5

/** Zoom after `steps` key presses from `current`, kept within the map's limits. */
export function steppedZoom(current: number, steps: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, current + steps * ZOOM_STEP))
}
