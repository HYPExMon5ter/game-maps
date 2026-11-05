// A game's checklist of key items, from `game.json`, linked to their pins.

import type { Pin } from './game'

/** An entry as `game.json` lists it. */
export interface ChecklistEntry {
  name: string
  /** Names of the pins that count (any one found ticks the entry). */
  pins: string[]
}

export interface ChecklistRow {
  name: string
  done: boolean
  /** The pins that count; empty when this map doesn't have them. */
  pins: Pin[]
}

export function checklistRows(entries: ChecklistEntry[], pins: Pin[], found: Iterable<string>): ChecklistRow[] {
  const isFound = new Set(found)
  return entries.map((entry) => {
    const names = new Set(entry.pins)
    const matching = pins.filter((p) => !p.custom && names.has(p.name))
    return { name: entry.name, done: matching.some((p) => isFound.has(p.id)), pins: matching }
  })
}
