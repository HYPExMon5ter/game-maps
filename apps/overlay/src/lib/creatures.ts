// Creatures around the player (enemies, animals, companions) as the map shows them.

import type { Creature, CreatureKind } from './protocol'

/** Dot colors on the map. */
export const CREATURE_COLORS: Record<CreatureKind, string> = {
  hostile: '#ef4444',
  animal: '#f5b942',
  friendly: '#4ade80',
}

/** Display name from the game's type name: "MuddyFemale" → "Muddy female". */
export function creatureName(name: string): string {
  const words = name
    .replace(/_/g, ' ')
    .replace(/([a-z])([A-Z0-9])/g, '$1 $2')
    .trim()
    .split(/\s+/)
  return words.map((w, i) => (i === 0 ? w : w.toLowerCase())).join(' ')
}

/**
 * The creatures to draw as dots, per the map settings. Friendly ones are companions,
 * drawn and listed with the players (see `lib/movers`).
 */
export function visibleCreatures(creatures: Creature[], show: boolean, animals: boolean): Creature[] {
  if (!show) return []
  return creatures.filter((c) => c.kind !== 'friendly' && (animals || c.kind !== 'animal'))
}
