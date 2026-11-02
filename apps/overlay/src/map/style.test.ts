import { validateStyleMin } from '@maplibre/maplibre-gl-style-spec'
import { describe, expect, it } from 'vitest'
import type { GameDefinition } from '../lib/game'
import { buildStyle } from './style'

const game: GameDefinition = {
  id: 'test',
  name: 'Test',
  process: 'test.exe',
  unitsPerMeter: 1,
  defaultHotkeys: [],
  categories: [],
  maps: [
    {
      id: 'surface',
      name: 'Surface',
      layers: ['surface', 'caves'],
      background: '#2e5c80',
      tiles: { url: 'tiles/{z}/{x}/{y}.webp', tileSize: 256, minNativeZoom: 2, maxNativeZoom: 7, maxZoom: 10 },
      projection: { east: 'x', north: 'z', up: 'y', minEast: -2000, maxEast: 2000, minNorth: -2000, maxNorth: 2000, tileZoom: 2, tileX: 1, tileY: 1 },
    },
  ],
}

describe('buildStyle', () => {
  it('produces a style MapLibre accepts', () => {
    const errors = validateStyleMin(buildStyle(game, game.maps[0], 'http://localhost'))
    expect(errors.map((e) => e.message)).toEqual([])
  })
})

describe('buildStyle with cave imagery', () => {
  it('adds a hidden cave raster layer that MapLibre accepts', () => {
    const map = { ...game.maps[0], layerTiles: { caves: { ...game.maps[0].tiles, url: 'tiles/caves/{z}/{x}/{y}.webp' } } }
    const style = buildStyle(game, map, 'http://localhost')
    expect(validateStyleMin(style).map((e) => e.message)).toEqual([])
    expect(style.layers.find((l) => l.id === 'caves')?.layout).toEqual({ visibility: 'none' })
  })
})
