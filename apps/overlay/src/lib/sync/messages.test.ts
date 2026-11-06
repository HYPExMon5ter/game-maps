import { describe, expect, it } from 'vitest'
import type { Pin } from '../game'
import { friendPin, parseMessage, toSharedPin } from './messages'

const at = { x: 1, y: 2, z: 3 }

describe('parseMessage', () => {
  it('accepts well-formed messages', () => {
    expect(parseMessage({ type: 'hello', version: 1, name: 'Ana', host: true })).toEqual({ type: 'hello', version: 1, name: 'Ana', host: true })
    expect(parseMessage({ type: 'ping', position: at, layer: 'caves' })).toEqual({ type: 'ping', position: at, layer: 'caves' })
    expect(parseMessage({ type: 'track', target: { name: 'Shovel', position: at, layer: 'surface', pinId: 'p1' } })).toEqual({
      type: 'track',
      target: { name: 'Shovel', position: at, layer: 'surface', pinId: 'p1' },
    })
    expect(parseMessage({ type: 'pins', pins: [{ id: 'a', name: 'Base', position: at, layer: 'surface', category: 'custom', extra: 1 }] })).toEqual({
      type: 'pins',
      pins: [{ id: 'a', name: 'Base', position: at, layer: 'surface', category: 'custom' }],
    })
  })

  it('rejects anything else', () => {
    expect(parseMessage(null)).toBeNull()
    expect(parseMessage('ping')).toBeNull()
    expect(parseMessage({ type: 'nope' })).toBeNull()
    expect(parseMessage({ type: 'ping', position: { x: 1, y: 2 }, layer: 'surface' })).toBeNull()
    expect(parseMessage({ type: 'ping', position: { x: Infinity, y: 0, z: 0 }, layer: 'surface' })).toBeNull()
    expect(parseMessage({ type: 'ping', position: at, layer: 'space' })).toBeNull()
    expect(parseMessage({ type: 'track', target: { name: 'x'.repeat(5000), position: at, layer: 'surface' } })).toBeNull()
    expect(parseMessage({ type: 'pins', pins: 'many' })).toBeNull()
  })

  it('keeps the good pins of a list with a bad one', () => {
    const good = { id: 'b', name: 'Base', position: at, layer: 'surface', category: 'custom' }
    expect(parseMessage({ type: 'pins', pins: [{ id: 'a' }, good] })).toEqual({ type: 'pins', pins: [good] })
  })
})

describe('shared pins', () => {
  const pin: Pin = { id: 'custom-1', category: 'custom', name: 'Base', position: at, custom: true, shared: true, icon: 'user:me.png', color: '#fff' }

  it('leave out icons only this computer has', () => {
    expect(toSharedPin(pin)).toEqual({ id: 'custom-1', name: 'Base', position: at, layer: 'surface', category: 'custom', color: '#fff' })
  })

  it('show up as the friend’s pins, credited to them', () => {
    const p = friendPin('peer1', 'Ana', toSharedPin({ ...pin, description: 'Our base' }))
    expect(p.id).toBe('friend:peer1:custom-1')
    expect(p.category).toBe('friends')
    expect(p.description).toBe('Shared by Ana\nOur base')
    expect(p.custom).toBeUndefined()
  })
})
