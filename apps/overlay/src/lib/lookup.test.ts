import { describe, expect, it } from 'vitest'
import type { Pin } from './game'
import { foundSet, pinById } from './lookup'

const pin = (id: string, name = id): Pin => ({ id, name, category: 'custom', position: { x: 0, y: 0, z: 0 } })

describe('lookup', () => {
  it('builds the found set once per list', () => {
    const found = ['a', 'b']
    expect(foundSet(found).has('a')).toBe(true)
    expect(foundSet(found).has('c')).toBe(false)
    expect(foundSet(found)).toBe(foundSet(found))
  })

  it('follows a new found list', () => {
    const found = ['a']
    expect(foundSet(found).has('b')).toBe(false)
    expect(foundSet([...found, 'b']).has('b')).toBe(true)
  })

  it('finds pins by id, the first when ids repeat', () => {
    const pins = [pin('a', 'first'), pin('b'), pin('a', 'second')]
    expect(pinById(pins, 'a')?.name).toBe('first')
    expect(pinById(pins, 'b')).toBe(pins[1])
    expect(pinById(pins, 'missing')).toBeUndefined()
  })

  it('follows a new pin list', () => {
    const pins = [pin('a')]
    expect(pinById(pins, 'b')).toBeUndefined()
    expect(pinById([...pins, pin('b')], 'b')?.id).toBe('b')
  })
})
