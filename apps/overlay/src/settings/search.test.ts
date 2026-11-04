import { describe, expect, it } from 'vitest'
import { matchesQuery, queryWords } from './search'

describe('settings search', () => {
  it('splits a query into lowercase words', () => {
    expect(queryWords('  Mini-Map  opacity! ')).toEqual(['mini', 'map', 'opacity'])
    expect(queryWords('')).toEqual([])
  })

  it('needs every word, at the start of a word, in any order', () => {
    const texts = ['Include animals', 'Wildlife as well as enemies', 'creatures deer']
    expect(matchesQuery(queryWords('anim'), texts)).toBe(true)
    expect(matchesQuery(queryWords('deer include'), texts)).toBe(true)
    expect(matchesQuery(queryWords('animals key'), texts)).toBe(false)
    expect(matchesQuery(queryWords('nimals'), texts)).toBe(false)
    expect(matchesQuery([], texts)).toBe(true)
  })

  it('ignores missing text', () => {
    expect(matchesQuery(['size'], ['Pin size', undefined])).toBe(true)
  })
})
