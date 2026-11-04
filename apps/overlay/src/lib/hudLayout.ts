// Geometry of the HUD shown while playing (mini-map and/or tracking compass).

import { minimapSize } from './minimap'

export const COMPASS_HEIGHT = 70
export const HUD_GAP = 8
/** Margin of the HUD from the screen edges. */
export const HUD_MARGIN = 24

/** Logical size of the corner HUD (mini-map and/or compass). */
export function hudSize(minimap: boolean, tracking: boolean, hud: { width: number; height: number }) {
  const size = minimapSize(hud, window.screen)
  return {
    width: minimap ? size.width : 280,
    height: Math.max((minimap ? size.height : 0) + (tracking ? COMPASS_HEIGHT + (minimap ? HUD_GAP : 0) : 0), 1),
  }
}
