import { beforeEach, describe, expect, it } from 'vitest'
import { EMPTY_USER_DATA, useStore } from '../state/store'
import { applyFilterSet, deleteFilterSet, matchingFilterSet, saveFilterSet, withFilterSet } from './filterSets'
import type { Category } from './game'

const category = (id: string, visible = true) => ({ id, visible }) as Category
const categories = [category('caves'), category('ammo'), category('fish', false)]
const filters = (hidden: string[] = [], shown: string[] = []) => ({ hiddenCategories: hidden, shownCategories: shown })

describe('withFilterSet', () => {
  it('adds a set', () => {
    expect(withFilterSet([], 'Looting', filters(['caves']))).toMatchObject([{ name: 'Looting', hiddenCategories: ['caves'] }])
  })

  it('replaces a set of the same name, whatever the case', () => {
    const [first] = withFilterSet([], 'Looting', filters(['caves']))
    const sets = withFilterSet([first], 'looting', filters(['ammo']))
    expect(sets).toHaveLength(1)
    expect(sets[0]).toMatchObject({ id: first.id, name: 'looting', hiddenCategories: ['ammo'] })
  })
})

describe('matchingFilterSet', () => {
  const sets = withFilterSet([], 'No caves', filters(['caves']))

  it('compares what is shown, not how it was filtered', () => {
    expect(matchingFilterSet(sets, categories, filters(['caves', 'fish']))?.name).toBe('No caves')
  })

  it('finds nothing when other categories are shown', () => {
    expect(matchingFilterSet(sets, categories, filters())).toBeUndefined()
    expect(matchingFilterSet(sets, categories, filters(['caves'], ['fish']))).toBeUndefined()
  })
})

describe('saved sets in the store', () => {
  beforeEach(() => useStore.setState({ user: { ...EMPTY_USER_DATA, hiddenCategories: ['caves'], shownCategories: ['fish'] } }))

  it('saves, applies and deletes', () => {
    saveFilterSet('Fishing')
    const [set] = useStore.getState().user.filterSets
    useStore.setState((s) => ({ user: { ...s.user, hiddenCategories: [], shownCategories: [] } }))
    applyFilterSet(set)
    expect(useStore.getState().user).toMatchObject({ hiddenCategories: ['caves'], shownCategories: ['fish'] })
    deleteFilterSet(set.id)
    expect(useStore.getState().user.filterSets).toEqual([])
  })
})
