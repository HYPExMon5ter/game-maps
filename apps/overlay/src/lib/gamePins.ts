// Which of the player's pins go on the game's own map (see `Capabilities.game_map`).

import type { GamePin } from './backend'
import type { Pin } from './game'

/** The player's own pins, without the one the app moves around by itself. */
export function gamePinsOf(pins: Pin[], deathPin: string): GamePin[] {
  return pins.filter((p) => p.id !== deathPin).map((p) => ({ name: p.name, position: p.position }))
}
