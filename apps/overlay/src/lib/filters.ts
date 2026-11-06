// What's shown on the map: groups of categories, and categories split into the kinds of
// thing in them ("Ore" into iron, copper...). A filter is a category id, or
// `<category>/<kind>` for one kind; a kind follows its category until it's set by itself.

import type { Category, GameDefinition, Pin } from './game'

export interface Filters {
  hiddenCategories: string[]
  shownCategories: string[]
}

/** One line under a group: a whole category, or one kind in it. */
export interface FilterItem {
  key: string
  category: Category
  label: string
  icon?: string
  color?: string
  total: number
  found: number
}

export interface FilterGroup {
  id: string
  name: string
  categories: Category[]
  items: FilterItem[]
  total: number
  found: number
}

/** "Only this", and what to go back to. */
export interface OnlyFilter {
  key: string
  label: string
  before: Filters
}

/** Holds whatever a game's groups don't: the player's own pins, friends' pins and so on. */
const OWN_PINS_GROUP = { id: 'own-pins', name: 'My Pins' }

export function isCategoryVisible(category: Category | undefined, filters: Filters): boolean {
  if (!category) return true
  if (filters.hiddenCategories.includes(category.id)) return false
  if (filters.shownCategories.includes(category.id)) return true
  return category.visible
}

/** The kind of thing a pin is, in a category that's split. */
function kindOf(category: Category, pin: Pin): string {
  return category.mergeNames?.[pin.name] ?? pin.name
}

const kindKey = (category: Category, kind: string) => `${category.id}/${kind}`

function isKeyVisible(key: string, category: Category, filters: Filters): boolean {
  if (filters.hiddenCategories.includes(key)) return false
  if (filters.shownCategories.includes(key)) return true
  return isCategoryVisible(category, filters)
}

export function isPinVisible(category: Category | undefined, pin: Pin, filters: Filters): boolean {
  if (!category?.split) return isCategoryVisible(category, filters)
  return isKeyVisible(kindKey(category, kindOf(category, pin)), category, filters)
}

export function isItemVisible(item: FilterItem, filters: Filters): boolean {
  return isKeyVisible(item.key, item.category, filters)
}

/** `filters` with one item set. */
export function withItem({ hiddenCategories, shownCategories }: Filters, key: string, visible: boolean): Filters {
  const others = (keys: string[]) => keys.filter((k) => k !== key)
  return visible ? { hiddenCategories: others(hiddenCategories), shownCategories: [...others(shownCategories), key] } : { hiddenCategories: [...others(hiddenCategories), key], shownCategories: others(shownCategories) }
}

/** `filters` with whole categories set, and any kinds in them back to following along. */
export function withCategories({ hiddenCategories, shownCategories }: Filters, ids: string[], visible: boolean): Filters {
  const others = (keys: string[]) => keys.filter((k) => !ids.includes(k.split('/')[0]))
  return visible ? { hiddenCategories: others(hiddenCategories), shownCategories: [...others(shownCategories), ...ids] } : { hiddenCategories: [...others(hiddenCategories), ...ids], shownCategories: others(shownCategories) }
}

/** Filters that show `key` and nothing else. */
export function onlyFilters(categories: Category[], key: string): Filters {
  return { hiddenCategories: categories.map((c) => c.id).filter((id) => id !== key), shownCategories: [key] }
}

function itemsOf(category: Category, pins: Pin[], found: Set<string>): FilterItem[] {
  const count = (items: Map<string, FilterItem>, key: string, label: string, pin: Pin) => {
    const item = items.get(key) ?? { key, category, label, icon: pin.icon, color: pin.color, total: 0, found: 0 }
    item.total++
    if (found.has(pin.id)) item.found++
    items.set(key, item)
  }
  const items = new Map<string, FilterItem>()
  for (const pin of pins) {
    if (!category.split) count(items, category.id, category.name, pin)
    else count(items, kindKey(category, kindOf(category, pin)), kindOf(category, pin), pin)
  }
  const kinds = [...items.values()].sort((a, b) => b.total - a.total || a.label.localeCompare(b.label))
  // With nothing in it yet, as in a world that's still being explored, it's one line.
  if (category.split && kinds.length > 0) return kinds
  return kinds.length > 0 ? kinds.map((k) => ({ ...k, icon: undefined, color: undefined })) : [{ key: category.id, category, label: category.name, total: 0, found: 0 }]
}

/** The game's categories as the sidebar lists them, with what's in each. */
export function filterGroups(game: GameDefinition, pins: Pin[], found: Set<string>): FilterGroup[] {
  const byCategory = new Map<string, Pin[]>()
  for (const pin of pins) byCategory.set(pin.category, [...(byCategory.get(pin.category) ?? []), pin])
  const categories = new Map(game.categories.map((c) => [c.id, c]))
  const grouped = new Set((game.groups ?? []).flatMap((g) => g.categories))
  const rest = game.categories.filter((c) => !grouped.has(c.id)).map((c) => c.id)
  const groups = [...(game.groups ?? []), ...(rest.length > 0 ? [{ ...OWN_PINS_GROUP, categories: rest }] : [])]
  return groups.map(({ id, name, categories: ids }) => {
    const members = ids.flatMap((c) => categories.get(c) ?? [])
    const items = members.flatMap((c) => itemsOf(c, byCategory.get(c.id) ?? [], found))
    return { id, name, categories: members, items, total: items.reduce((n, i) => n + i.total, 0), found: items.reduce((n, i) => n + i.found, 0) }
  })
}
