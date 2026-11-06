// MapLibre style for a game map: raster base, track line, and pin symbols.

import type { ExpressionSpecification, Map as MlMap, StyleSpecification } from 'maplibre-gl'
import { tileUrl, type GameDefinition, type GameMap } from '../lib/game'
import { mapBounds, mapZoom } from '../lib/projection'

/** Style layer with the underground imagery (see `GameMap.layerTiles`). */
export const CAVES_LAYER = 'caves'

/** Relative zoom (see `mapZoom`) from which pin names are shown. */
export const LABEL_ZOOM = 2.5

const FONT = ['Open Sans Semibold']

/** How pins are drawn; user settings (see `Settings.pinSize` / `declutter`). */
export interface PinDisplay {
  /** Multiplies the zoom-based badge size (1 = default). */
  size: number
  /** Overlapping badges give way to more important pins, which leave a dot behind. */
  declutter: boolean
}

export const DEFAULT_PIN_DISPLAY: PinDisplay = { size: 1, declutter: true }

/**
 * Badge scale by zoom (the badge image is 44 px): small when the whole map is in view,
 * growing as you zoom in. Relative zoom stops, see `mapZoom`.
 */
const BADGE_SCALE: [number, number][] = [
  [-1, 0.36],
  [1, 0.46],
  [3, 0.62],
  [5, 0.8],
]
/** Selected pins are drawn larger. */
const SELECTED_SCALE = 1.3

/** `icon-size` for pins (and live markers) at the given user size. */
export function pinIconSize(map: GameMap, size: number, selectable = true): ExpressionSpecification {
  const stops = BADGE_SCALE.flatMap(([zoom, scale]) => {
    const base = scale * size
    return [mapZoom(map.projection, zoom), selectable ? ['case', ['get', 'selected'], base * SELECTED_SCALE, base] : base]
  })
  return ['interpolate', ['linear'], ['zoom'], ...stops] as ExpressionSpecification
}

/** Radius of the dot left by a pin whose badge gave way to a more important one. */
function dotRadius(map: GameMap, size: number): ExpressionSpecification {
  const z = (r: number) => mapZoom(map.projection, r)
  return ['interpolate', ['linear'], ['zoom'], z(-1), 2 * size, z(3), 3 * size, z(5), 4 * size]
}

/** Applies the user's pin size and declutter settings to a loaded map. */
export function applyPinDisplay(ml: MlMap, map: GameMap, display: PinDisplay) {
  ml.setLayoutProperty('pins', 'icon-size', pinIconSize(map, display.size))
  ml.setLayoutProperty('pins', 'icon-allow-overlap', !display.declutter)
  ml.setLayoutProperty('pins', 'icon-ignore-placement', !display.declutter)
  ml.setLayoutProperty('markers', 'icon-size', pinIconSize(map, display.size, false))
  ml.setLayoutProperty('pin-dots', 'visibility', display.declutter ? 'visible' : 'none')
  ml.setPaintProperty('pin-dots', 'circle-radius', dotRadius(map, display.size))
}

/** `assetOrigin` serves the label fonts (`public/fonts`). */
export function buildStyle(game: GameDefinition, map: GameMap, assetOrigin: string, worldId?: string): StyleSpecification {
  const z = (relative: number) => mapZoom(map.projection, relative)
  const caves = map.layerTiles?.caves
  return {
    version: 8,
    glyphs: `${assetOrigin}/fonts/{fontstack}/{range}.pbf`,
    sources: {
      base: {
        type: 'raster',
        tiles: [tileUrl(game, map, map.tiles, worldId)],
        tileSize: map.tiles.tileSize,
        minzoom: map.tiles.minNativeZoom,
        maxzoom: map.tiles.maxNativeZoom,
        bounds: insetBounds(mapBounds(map.projection)),
      },
      pins: { type: 'geojson', data: { type: 'FeatureCollection', features: [] } },
      track: { type: 'geojson', data: { type: 'FeatureCollection', features: [] } },
      trail: { type: 'geojson', data: { type: 'FeatureCollection', features: [] } },
      markers: { type: 'geojson', data: { type: 'FeatureCollection', features: [] } },
      creatures: { type: 'geojson', data: { type: 'FeatureCollection', features: [] } },
      pings: { type: 'geojson', data: { type: 'FeatureCollection', features: [] } },
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
        id: 'trail-line',
        type: 'line',
        source: 'trail',
        layout: { 'line-cap': 'round', 'line-join': 'round' },
        paint: { 'line-color': '#7dd3fc', 'line-width': 2, 'line-opacity': 0.55 },
      },
      {
        id: 'track-line',
        type: 'line',
        source: 'track',
        paint: { 'line-color': '#ffe08a', 'line-width': 2.5, 'line-dasharray': [2, 1.5], 'line-opacity': 0.9 },
      },
      {
        // Every pin as a small dot, under the badges: where decluttering hides a badge,
        // its dot still shows where the pin is.
        id: 'pin-dots',
        type: 'circle',
        source: 'pins',
        paint: {
          'circle-color': ['get', 'color'],
          'circle-radius': dotRadius(map, DEFAULT_PIN_DISPLAY.size),
          'circle-stroke-color': 'rgba(0,0,0,0.6)',
          'circle-stroke-width': 1,
          'circle-opacity': ['*', ['case', ['get', 'found'], 0.4, 1], ['case', ['get', 'otherLayer'], 0.4, 1]],
          'circle-stroke-opacity': ['*', ['case', ['get', 'found'], 0.4, 1], ['case', ['get', 'otherLayer'], 0.4, 1]],
        },
      },
      {
        id: 'pins',
        type: 'symbol',
        source: 'pins',
        layout: {
          'icon-image': ['get', 'icon'],
          'icon-size': pinIconSize(map, DEFAULT_PIN_DISPLAY.size),
          'icon-allow-overlap': !DEFAULT_PIN_DISPLAY.declutter,
          'icon-ignore-placement': !DEFAULT_PIN_DISPLAY.declutter,
          'icon-padding': 1,
          // Placed first (and so kept when badges collide): the selected pin, then by
          // category order (game.json lists the important ones first); pins on the other
          // layer (surface vs. underground) last.
          'symbol-sort-key': ['case', ['get', 'selected'], -1, ['+', ['get', 'rank'], ['case', ['get', 'otherLayer'], 1000, 0]]],
          'symbol-z-order': 'source',
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
        // Creatures around the player: colored dots, named when zoomed in.
        id: 'creatures',
        type: 'circle',
        source: 'creatures',
        paint: {
          'circle-color': ['get', 'color'],
          'circle-radius': ['interpolate', ['linear'], ['zoom'], z(0), 3, z(LABEL_ZOOM), 5],
          'circle-stroke-color': 'rgba(0,0,0,0.75)',
          'circle-stroke-width': 1.2,
        },
      },
      {
        id: 'creature-labels',
        type: 'symbol',
        source: 'creatures',
        minzoom: z(LABEL_ZOOM),
        layout: {
          'text-field': ['get', 'name'],
          'text-font': FONT,
          'text-size': 11,
          'text-offset': [0, 0.9],
          'text-anchor': 'top',
          'text-optional': true,
        },
        paint: { 'text-color': ['get', 'color'], 'text-halo-color': 'rgba(0,0,0,0.8)', 'text-halo-width': 1.3 },
      },
      {
        // Spots players pinged (see lib/sync): rings that fade as the ping ages.
        id: 'pings',
        type: 'circle',
        source: 'pings',
        paint: {
          'circle-radius': 11,
          'circle-color': 'rgba(157, 78, 221, 0.25)',
          'circle-stroke-color': '#ffffff',
          'circle-stroke-width': 2.5,
          'circle-opacity': ['get', 'fade'],
          'circle-stroke-opacity': ['get', 'fade'],
        },
      },
      {
        id: 'ping-labels',
        type: 'symbol',
        source: 'pings',
        layout: {
          'text-field': ['get', 'label'],
          'text-font': FONT,
          'text-size': 12,
          'text-offset': [0, 1.4],
          'text-anchor': 'top',
          'text-allow-overlap': true,
        },
        paint: { 'text-color': '#ffffff', 'text-halo-color': 'rgba(0,0,0,0.8)', 'text-halo-width': 1.4, 'text-opacity': ['get', 'fade'] },
      },
      {
        // Live markers from the game's own GPS (placed locators, moved vehicles, …).
        id: 'markers',
        type: 'symbol',
        source: 'markers',
        layout: {
          'icon-image': ['get', 'icon'],
          'icon-size': pinIconSize(map, DEFAULT_PIN_DISPLAY.size, false),
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
