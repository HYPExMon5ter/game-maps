import { describe, expect, it } from 'vitest'
import { EMPTY_USER_DATA, useStore } from '../state/store'
import { bagMoved, clearDeathPin, markBag, markDeath } from './actions'

const at = { x: 10, y: 2, z: 30 }

describe('bagMoved', () => {
  it('is true for a new bag and a real move', () => {
    expect(bagMoved(undefined, at)).toBe(true)
    expect(bagMoved(at, { ...at, x: 15 })).toBe(true)
  })

  it('ignores the bag settling', () => {
    expect(bagMoved(at, { ...at, y: 2.4 })).toBe(false)
  })
})

describe('the death pin', () => {
  const pins = () => useStore.getState().user.customPins

  it('moves from where the player died to their bag, then goes', () => {
    useStore.setState({ user: EMPTY_USER_DATA })
    markDeath({ position: { x: 0, y: 0, z: 0 }, layer: 'surface' } as never)
    expect(pins()).toMatchObject([{ name: 'Where you died', icon: 'builtin:skull' }])
    markBag(at)
    expect(pins()).toHaveLength(1)
    expect(pins()[0]).toMatchObject({ name: 'Your bag', icon: 'builtin:backpack', position: at })
    clearDeathPin()
    expect(pins()).toEqual([])
  })
})
