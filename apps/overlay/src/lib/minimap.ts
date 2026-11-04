// Mini-map size limits.

import type { HudSettings } from '../settings/model'

export const MIN_MINIMAP_SIZE = 160

type Size = { width: number; height: number }

/** Largest mini-map on a screen: half its width and half its height. */
export function maxMinimapSize(screen: Size): Size {
  return { width: Math.round(screen.width / 2), height: Math.round(screen.height / 2) }
}

/** Mini-map size, capped so a size picked on a bigger screen still fits this one. */
export function minimapSize(hud: Pick<HudSettings, 'width' | 'height'>, screen: Size): Size {
  const max = maxMinimapSize(screen)
  return { width: Math.min(hud.width, max.width), height: Math.min(hud.height, max.height) }
}
