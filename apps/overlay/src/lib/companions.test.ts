import { describe, expect, it } from 'vitest'
import { companionAlerts, companionState, companionStatus, orderMenu } from './companions'
import type { CompanionStatus, WorldSnapshot } from './protocol'

const order = (group: string, label: string, id: string) => ({ id, group, label })

const kelvin: CompanionStatus = { id: 'kelvin', name: 'Kelvin', position: null, alive: true, injured: false, activity: null, stats: [] }

describe('orderMenu', () => {
  it('groups orders in the order they came', () => {
    const menu = orderMenu([order('Basics', 'Follow me', 'follow'), order('Stay', 'Here', 'stay:0'), order('Basics', 'Take a break', 'break')])
    expect(menu.groups.map((g) => [g.title, g.orders.map((o) => o.id)])).toEqual([
      ['Basics', ['follow', 'break']],
      ['Stay', ['stay:0']],
    ])
    expect(menu.fetch).toEqual([])
  })

  it('keeps fetching apart, per item', () => {
    const menu = orderMenu([order('Get logs', 'Bring to me', 'get:2:1'), order('Get logs', 'Drop here', 'get:2:0'), order('Get rocks', 'Bring to me', 'get:1:1')])
    expect(menu.groups).toEqual([])
    expect(menu.fetch.map((f) => [f.item, f.drops.map((d) => d.id)])).toEqual([
      ['logs', ['get:2:1', 'get:2:0']],
      ['rocks', ['get:1:1']],
    ])
  })
})

describe('companionStatus', () => {
  it('finds a companion by name', () => {
    const snapshot = { companions: [kelvin] } as WorldSnapshot
    expect(companionStatus(snapshot, 'Kelvin')).toBe(kelvin)
    expect(companionStatus(snapshot, 'Virginia')).toBeUndefined()
    expect(companionStatus(null, 'Kelvin')).toBeUndefined()
  })
})

describe('companionState', () => {
  it('says what matters most', () => {
    expect(companionState(kelvin)).toBe('Idle')
    expect(companionState({ ...kelvin, activity: 'Logs' })).toBe('Logs')
    expect(companionState({ ...kelvin, activity: 'Logs', injured: true })).toBe('Injured, needs helping up')
    expect(companionState({ ...kelvin, alive: false, injured: true })).toBe('Dead')
  })
})

describe('companionAlerts', () => {
  const kelvin = (fullness: number, extra: Partial<CompanionStatus> = {}): CompanionStatus => ({ id: 'kelvin', name: 'Kelvin', position: null, activity: null, alive: true, injured: false, stats: [{ name: 'Fullness', value: fullness, max: 100 }], ...extra })

  it('warns once, and again only after recovering', () => {
    const warned = new Set<string>()
    expect(companionAlerts([kelvin(20)], warned)).toEqual(["Kelvin's fullness is low"])
    expect(companionAlerts([kelvin(10)], warned)).toEqual([])
    expect(companionAlerts([kelvin(30)], warned)).toEqual([])
    expect(companionAlerts([kelvin(20)], warned)).toEqual([])
    expect(companionAlerts([kelvin(50)], warned)).toEqual([])
    expect(companionAlerts([kelvin(20)], warned)).toEqual(["Kelvin's fullness is low"])
  })

  it('says when they die, and leaves their stats alone then', () => {
    expect(companionAlerts([kelvin(0, { alive: false })], new Set())).toEqual(['Kelvin died'])
  })

  it('ignores stats that are fine to run out', () => {
    expect(companionAlerts([kelvin(90, { stats: [{ name: 'Morale', value: 0, max: 100 }] })], new Set())).toEqual([])
  })
})
