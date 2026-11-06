// A game's checklist of key items, from `game.json`, linked to their pins.

import type { Pin } from './game'

/** An entry as `game.json` lists it. */
export interface ChecklistEntry {
  name: string
  /** Names of the pins that count (any one found ticks the entry). */
  pins: string[]
  /** The game's own milestone that ticks it, in place of finding a pin. */
  key?: string
  /** What it's good for. */
  note?: string
}

export interface ChecklistRow {
  name: string
  done: boolean
  note?: string
  /** The pins that count; empty when this map doesn't have them. */
  pins: Pin[]
}

export function checklistRows(entries: ChecklistEntry[], pins: Pin[], found: Iterable<string>, progress: string[] = []): ChecklistRow[] {
  const isFound = new Set(found)
  return entries.map((entry) => {
    const names = new Set(entry.pins)
    const matching = pins.filter((p) => !p.custom && names.has(p.name))
    const done = entry.key ? progress.includes(entry.key) : matching.some((p) => isFound.has(p.id))
    return { name: entry.name, done, note: entry.note, pins: matching }
  })
}
