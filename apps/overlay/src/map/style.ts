// MapLibre style for a game map: raster base, track line, and pin symbols.

import type { StyleSpecification } from 'maplibre-gl'
import { tileUrl, type GameDefinition, type GameMap } from '../lib/game'
import { mapBounds, mapZoom } from '../lib/projection'

/** Style layer with the underground imagery (see `GameMap.layerTiles`). */
export const CAVES_LAYER = 'caves'

/** Relative zoom (see `mapZoom`) from which pin names are shown. */
export const LABEL_ZOOM = 2.5

const FONT = ['Open Sans Semibold']

/** `assetOrigin` serves the label fonts (`public/fonts`). */
export function buildStyle(game: GameDefinition, map: GameMap, assetOrigin: string): StyleSpecification {
  const z = (relative: number) => mapZoom(map.projection, relative)
  const caves = map.layerTiles?.caves
  return {
    version: 8,
    glyphs: `${assetOrigin}/fonts/{fontstack}/{range}.pbf`,
    sources: {
      base: {
        type: 'raster',
        tiles: [tileUrl(game, map)],
        tileSize: map.tiles.tileSize,
        minzoom: map.tiles.minNativeZoom,
        maxzoom: map.tiles.maxNativeZoom,
        bounds: insetBounds(mapBounds(map.projection)),
      },
      pins: { type: 'geojson', data: { type: 'FeatureCollection', features: [] } },
      track: { type: 'geojson', data: { type: 'FeatureCollection', features: [] } },
      markers: { type: 'geojson', data: { type: 'FeatureCollection', features: [] } },
      ...(caves && {
        caves: {
          type: 'raster',
          tiles: [tileUrl(game, map, caves)],
          tileSize: caves.tileSize,
          minzoom: caves.minNativeZoom,
          maxzoom: caves.maxNativeZoom,
          bounds: insetBounds(mapBounds(map.projection)),
        },
      }),
    },
    layers: [
      { id: 'background', type: 'background', paint: { 'background-color': map.background } },
      { id: 'base', type: 'raster', source: 'base', paint: { 'raster-fade-duration': 0, 'raster-resampling': 'linear' } },
      // Cave layouts, shown only while viewing the underground layer.
      ...(caves
        ? [
            {
              id: CAVES_LAYER,
              type: 'raster' as const,
              source: 'caves',
              layout: { visibility: 'none' as const },
              paint: { 'raster-fade-duration': 0, 'raster-resampling': 'linear' as const },
            },
          ]
        : []),
      {
        id: 'track-line',
        type: 'line',
        source: 'track',
        paint: { 'line-color': '#ffe08a', 'line-width': 2.5, 'line-dasharray': [2, 1.5], 'line-opacity': 0.9 },
      },
      {
        id: 'pins',
        type: 'symbol',
        source: 'pins',
        layout: {
          'icon-image': ['get', 'icon'],
          'icon-size': [
            'interpolate',
            ['linear'],
            ['zoom'],
            z(-1),
            ['case', ['get', 'selected'], 0.85, 0.6],
            z(2),
            ['case', ['get', 'selected'], 1.1, 0.85],
            z(4),
            ['case', ['get', 'selected'], 1.3, 1],
          ],
          'icon-allow-overlap': true,
          'icon-ignore-placement': true,
          'symbol-sort-key': ['case', ['get', 'selected'], 2, ['get', 'otherLayer'], 0, 1],
          'text-field': ['step', ['zoom'], '', z(LABEL_ZOOM), ['case', ['get', 'otherLayer'], '', ['get', 'name']]],
          'text-font': FONT,
          'text-size': 12,
          'text-offset': [0, 1.5],
          'text-anchor': 'top',
          'text-optional': true,
        },
        paint: {
          // Pins on the other layer (underground vs. surface) stay visible but faded.
          'icon-opacity': ['*', ['case', ['get', 'found'], 0.4, 1], ['case', ['get', 'otherLayer'], 0.4, 1]],
          'text-color': '#ffffff',
          'text-halo-color': 'rgba(0,0,0,0.75)',
          'text-halo-width': 1.4,
          'text-opacity': ['case', ['get', 'found'], 0.5, 1],
        },
      },
      {
        // Live markers from the game's own GPS (placed locators, moved vehicles, …).
        id: 'markers',
        type: 'symbol',
        source: 'markers',
        layout: {
          'icon-image': ['get', 'icon'],
          'icon-size': ['interpolate', ['linear'], ['zoom'], z(-1), 0.6, z(2), 0.85, z(4), 1],
          'icon-allow-overlap': true,
          'icon-ignore-placement': true,
          'text-field': ['step', ['zoom'], '', z(LABEL_ZOOM), ['get', 'name']],
          'text-font': FONT,
          'text-size': 12,
          'text-offset': [0, 1.5],
          'text-anchor': 'top',
          'text-optional': true,
        },
        paint: { 'text-color': '#ffffff', 'text-halo-color': 'rgba(0,0,0,0.75)', 'text-halo-width': 1.4 },
      },
    ],
  }
}

/** Shrinks bounds slightly so MapLibre doesn't request neighbor tiles that only touch the edge. */
function insetBounds([w, s, e, n]: [number, number, number, number]): [number, number, number, number] {
  const dx = (e - w) * 1e-4
  const dy = (n - s) * 1e-4
  return [w + dx, s + dy, e - dx, n - dy]
}
