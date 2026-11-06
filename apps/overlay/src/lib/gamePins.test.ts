import { describe, expect, it } from 'vitest'
import { gamePinsOf } from './gamePins'
import type { Pin } from './game'

const pin = (id: string, name: string): Pin => ({ id, category: 'custom', name, position: { x: 1, y: 2, z: 3 }, custom: true })

describe('gamePinsOf', () => {
  it('sends names and positions', () => {
    expect(gamePinsOf([pin('a', 'Base')], 'death')).toEqual([{ name: 'Base', position: { x: 1, y: 2, z: 3 } }])
  })

  it('leaves out the death pin', () => {
    expect(gamePinsOf([pin('death', 'Your bag'), pin('a', 'Base')], 'death').map((p) => p.name)).toEqual(['Base'])
  })
})
