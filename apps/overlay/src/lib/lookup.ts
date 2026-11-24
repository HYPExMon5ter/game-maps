// Lookups into the pin and found lists. Both are asked far more often than the lists
// change (with every snapshot while tracking, for every row of a list), and a game can
// have 10,000 pins and thousands of found ones, so each list is indexed once and the
// index kept for as long as the list is.

import type { Pin } from './game'

const foundSets = new WeakMap<readonly string[], ReadonlySet<string>>()

/** The found ids as a set. */
export function foundSet(found: readonly string[]): ReadonlySet<string> {
  let set = foundSets.get(found)
  if (!set) {
    set = new Set(found)
    foundSets.set(found, set)
  }
  return set
}

const pinIndexes = new WeakMap<readonly Pin[], Map<string, Pin>>()

/** The pin with this id; the first one when several share it. */
export function pinById(pins: readonly Pin[], id: string): Pin | undefined {
  let index = pinIndexes.get(pins)
  if (!index) {
    index = new Map()
    for (const pin of pins) if (!index.has(pin.id)) index.set(pin.id, pin)
    pinIndexes.set(pins, index)
  }
  return index.get(id)
}
