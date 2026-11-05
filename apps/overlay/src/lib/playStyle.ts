// The first-run question: how the player wants the map to work. Each answer is only a
// starting point for settings they can change at any time.

import type { Settings } from '../settings/model'

export interface PlayStyle {
  id: string
  title: string
  text: string
  /** Shown first and marked as the suggestion. */
  recommended?: boolean
  settings: Pick<Settings, 'fogOfWar' | 'teleport'>
}

export const PLAY_STYLES: PlayStyle[] = [
  {
    id: 'discover',
    title: 'Discover as I play',
    text: "Clouds cover the map and clear where you've been, so nothing gets spoiled.",
    recommended: true,
    settings: { fogOfWar: true, teleport: false },
  },
  {
    id: 'full',
    title: 'Show the whole map',
    text: 'Every location and item is on the map from the start.',
    settings: { fogOfWar: false, teleport: false },
  },
  {
    id: 'teleport',
    title: 'Whole map and teleport',
    text: 'See everything, and jump to any spot on the map.',
    settings: { fogOfWar: false, teleport: true },
  },
]

/** The settings after choosing `style`. */
export function withPlayStyle(settings: Settings, style: PlayStyle): Settings {
  return { ...settings, ...style.settings, onboarding: { ...settings.onboarding, welcomed: true } }
}
