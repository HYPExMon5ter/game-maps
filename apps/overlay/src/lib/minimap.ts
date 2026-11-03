// Mini-map size limits and saved-settings migration.

import { DEFAULT_SETTINGS, type HudSettings } from '../state/store'

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

/** Saved HUD settings with defaults filled in. Older versions had a square mini-map with one `size`. */
export function migrateHud(saved: (Partial<HudSettings> & { size?: number }) | undefined): HudSettings {
  const { size, ...rest } = saved ?? {}
  const square = size === undefined ? {} : { width: size, height: size }
  return { ...DEFAULT_SETTINGS.hud, ...square, ...rest }
}
