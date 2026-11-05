import { describe, expect, it } from 'vitest'
import type { Pin } from './game'
import { exportPins, importPins, newPinsFrom } from './pinTransfer'

const base: Pin = { id: 'c1', category: 'custom', name: 'Base', position: { x: 1, y: 2, z: 3 }, layer: 'surface', custom: true, description: 'Home', color: '#fff' }

describe('pin files', () => {
  it('round-trips pins', () => {
    const [pin] = importPins(exportPins('sotf', [base]), 'sotf')
    expect(pin).toMatchObject({ name: 'Base', position: base.position, description: 'Home', color: '#fff' })
  })

  it('refuses what is not a pins file', () => {
    expect(() => importPins('not json', 'sotf')).toThrow("isn't a pins file")
    expect(() => importPins('{"pins":[]}', 'sotf')).toThrow("isn't a pins file")
  })

  it('refuses pins for another game', () => {
    expect(() => importPins(exportPins('other', [base]), 'sotf')).toThrow('another game')
  })

  it('drops invalid pins', () => {
    const file = JSON.parse(exportPins('sotf', [base]))
    file.pins.push({ name: 'Broken' })
    expect(importPins(JSON.stringify(file), 'sotf').map((p) => p.name)).toEqual(['Base'])
  })
})

describe('newPinsFrom', () => {
  it('gives imported pins ids of their own', () => {
    const [pin] = importPins(exportPins('sotf', [base]), 'sotf')
    expect(newPinsFrom([pin], [], () => 'new')).toMatchObject([{ id: 'new', custom: true, name: 'Base' }])
  })

  it('leaves out pins already there, so importing twice changes nothing', () => {
    const imported = importPins(exportPins('sotf', [base, { ...base, id: 'c2' }]), 'sotf')
    expect(newPinsFrom(imported, [base], () => 'new')).toEqual([])
    expect(newPinsFrom(imported, [], () => 'new')).toHaveLength(1)
  })
})
