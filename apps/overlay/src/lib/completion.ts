// Completion: how much of each collectible category has been found in the current world.

import type { Category, Pin } from './game'

export interface CategoryProgress {
  category: Category
  found: number
  total: number
  /** Pins not found yet. */
  left: Pin[]
}

export interface Progress {
  categories: CategoryProgress[]
  found: number
  total: number
}

/**
 * Progress over collectible categories (marked found automatically), plus any other
 * game category the player has marked something found in. Custom and shared pins don't
 * count.
 */
export function completion(pins: Pin[], categories: Category[], found: Iterable<string>): Progress {
  const isFound = new Set(found)
  const byCategory = new Map<string, Pin[]>()
  for (const pin of pins) {
    if (pin.custom) continue
    const list = byCategory.get(pin.category)
    if (list) list.push(pin)
    else byCategory.set(pin.category, [pin])
  }
  const out: CategoryProgress[] = []
  for (const category of categories) {
    const list = byCategory.get(category.id) ?? []
    const left = list.filter((p) => !isFound.has(p.id))
    const done = list.length - left.length
    if (list.length && (category.autoFound || done > 0)) out.push({ category, found: done, total: list.length, left })
  }
  return { categories: out, found: out.reduce((n, c) => n + c.found, 0), total: out.reduce((n, c) => n + c.total, 0) }
}

/** A whole percentage that only reads 100% when everything is done. */
export function percent(found: number, total: number): number {
  if (total === 0) return 0
  const p = Math.round((found / total) * 100)
  return p === 100 && found < total ? 99 : p
}
