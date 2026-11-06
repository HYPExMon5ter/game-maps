// Saved filter sets: named category filters to switch between, kept per game.

import { useStore } from '../state/store'
import { isCategoryVisible, type Filters } from './filters'
import type { Category } from './game'

export interface FilterSet extends Filters {
  id: string
  name: string
}

/** `sets` with the filters saved as `name`, replacing a set of that name. */
export function withFilterSet(sets: FilterSet[], name: string, { hiddenCategories, shownCategories }: Filters): FilterSet[] {
  const same = (s: FilterSet) => s.name.toLowerCase() === name.toLowerCase()
  const set = { id: sets.find(same)?.id ?? crypto.randomUUID(), name, hiddenCategories, shownCategories }
  return sets.some(same) ? sets.map((s) => (same(s) ? set : s)) : [...sets, set]
}

/** The saved set showing exactly the categories `filters` shows, if any. */
export function matchingFilterSet(sets: FilterSet[], categories: Category[], filters: Filters): FilterSet | undefined {
  const kinds = (keys: string[]) => keys.filter((k) => k.includes('/')).sort()
  const shown = (f: Filters) => JSON.stringify([categories.map((c) => isCategoryVisible(c, f)), kinds(f.hiddenCategories), kinds(f.shownCategories)])
  return sets.find((s) => shown(s) === shown(filters))
}

const setFilterSets = (filterSets: FilterSet[]) => useStore.setState((s) => ({ user: { ...s.user, filterSets } }))

/** Saves what's shown now as `name`. */
export function saveFilterSet(name: string) {
  const { user } = useStore.getState()
  setFilterSets(withFilterSet(user.filterSets, name, user))
}

export function applyFilterSet({ hiddenCategories, shownCategories }: FilterSet) {
  useStore.setState((s) => ({ user: { ...s.user, hiddenCategories, shownCategories, only: null } }))
}

export function deleteFilterSet(id: string) {
  setFilterSets(useStore.getState().user.filterSets.filter((s) => s.id !== id))
}
