import MiniSearch from 'minisearch'
import type { Category, Pin } from './game'

interface Doc {
  id: string
  name: string
  category: string
  description: string
}

/** Fuzzy, prefix-aware search over pin names, categories and descriptions. */
export class PinSearch {
  #index: MiniSearch<Doc>

  constructor(pins: Pin[], categories: Category[]) {
    const names = new Map(categories.map((c) => [c.id, c.name]))
    this.#index = new MiniSearch<Doc>({
      fields: ['name', 'category', 'description'],
      storeFields: ['id'],
      searchOptions: { prefix: true, fuzzy: 0.2, boost: { name: 3, category: 1.5 }, combineWith: 'AND' },
    })
    this.#index.addAll(
      pins.map((p) => ({
        id: p.id,
        name: p.name,
        category: names.get(p.category) ?? p.category,
        description: p.description ?? '',
      })),
    )
  }

  /** Pin ids ordered by relevance. */
  search(query: string): string[] {
    const q = query.trim()
    if (!q) return []
    return this.#index.search(q).map((r) => r.id as string)
  }
}
