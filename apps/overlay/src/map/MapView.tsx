import { Map as MlMap, type GeoJSONSource } from 'maplibre-gl'
import 'maplibre-gl/dist/maplibre-gl.css'
import './setup'
import { useEffect, useMemo, useRef, useState } from 'react'
import { events } from '../lib/backend'
import type { Pin } from '../lib/game'
import { LiveMarkerFilter, markerName } from '../lib/markers'
import { findMover } from '../lib/movers'
import { bearingOf, mapBounds, mapCenter, toLngLat, zoomToFit, type LngLat, type Projection } from '../lib/projection'
import type { Marker } from '../lib/protocol'
import { steppedZoom } from '../lib/zoom'
import { isCategoryVisible, selectAllPins, selectViewLayer, useStore } from '../state/store'
import { badgeKey, ensureBadge } from './badges'
import { mapHandle } from './handle'
import { PlayerMarkers } from './players'
import { buildStyle, CAVES_LAYER } from './style'

export interface MapContextMenuEvent {
  lngLat: LngLat
  x: number
  y: number
}

interface Props {
  variant: 'full' | 'mini'
  onContextMenu?: (e: MapContextMenuEvent) => void
  /** Screen space covered by interface panels; the island is fitted into the rest. */
  fitPadding?: Insets
}

/** Screen-space insets in CSS pixels. */
export interface Insets {
  top: number
  right: number
  bottom: number
  left: number
}

const NO_PADDING: Insets = { top: 0, right: 0, bottom: 0, left: 0 }
/** Smallest map area kept free when panels would otherwise cover nearly everything. */
const MIN_FREE_PX = 160

/** Padding that leaves at least `MIN_FREE_PX` of map visible in each direction. */
function usablePadding(p: Insets, width: number, height: number): Insets {
  const shrink = (a: number, b: number, size: number) => {
    const over = a + b + MIN_FREE_PX - size
    if (over <= 0) return [a, b]
    const k = Math.max(0, (size - MIN_FREE_PX) / (a + b || 1))
    return [a * k, b * k]
  }
  const [left, right] = shrink(p.left, p.right, width)
  const [top, bottom] = shrink(p.top, p.bottom, height)
  return { top, right, bottom, left }
}

/** Mini-map zoom at the default `hud.zoom` of 0: about 1 screen pixel per world unit. */
function miniBaseZoom(p: Projection): number {
  return zoomToFit(p, p.maxEast - p.minEast)
}

/** Shows the whole island in the part of the map not covered by panels. */
function fitIsland(m: MlMap, p: Projection, padding: Insets) {
  const [w, s, e, n] = mapBounds(p)
  const c = m.getContainer()
  m.fitBounds(
    [
      [w, s],
      [e, n],
    ],
    { padding: usablePadding(padding, c.clientWidth, c.clientHeight), animate: false },
  )
}

export function MapView({ variant, onContextMenu, fitPadding = NO_PADDING }: Props) {
  const containerRef = useRef<HTMLDivElement>(null)
  const [map, setMap] = useState<MlMap | null>(null)
  const game = useStore((s) => s.game)
  const gameMap = useStore((s) => s.map)
  const contextMenuRef = useRef(onContextMenu)
  const fitPaddingRef = useRef(fitPadding)
  useEffect(() => {
    contextMenuRef.current = onContextMenu
    fitPaddingRef.current = fitPadding
  }, [onContextMenu, fitPadding])

  // MapLibre can't initialize in a zero-size container; wait for real layout.
  const [hasSize, setHasSize] = useState(false)
  useEffect(() => {
    const el = containerRef.current
    if (!el) return
    const ro = new ResizeObserver(([entry]) => {
      if (entry.contentRect.width > 0 && entry.contentRect.height > 0) setHasSize(true)
    })
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  // Create the map once per game map/variant.
  useEffect(() => {
    const container = containerRef.current
    if (!container || !game || !gameMap || !hasSize) return
    const full = variant === 'full'
    const p = gameMap.projection
    const center = mapCenter(p)
    const m = new MlMap({
      container,
      style: buildStyle(game, gameMap, window.location.origin),
      center: [center.lng, center.lat],
      // Full map: fitted below; mini-map: the user's zoom.
      zoom: miniBaseZoom(p) + (full ? 0 : useStore.getState().settings.hud.zoom),
      minZoom: zoomToFit(p, 256),
      maxZoom: gameMap.tiles.maxZoom,
      renderWorldCopies: false,
      attributionControl: false,
      interactive: full,
      dragRotate: false,
      pitchWithRotate: false,
      touchPitch: false,
      fadeDuration: 0,
    })
    m.touchZoomRotate.disableRotation()
    m.keyboard.disable() // the overlay handles its own keys (M / Esc)

    m.on('load', () => setMap(m))
    if (full) {
      fitIsland(m, p, fitPaddingRef.current)
      mapHandle.current = m
      m.on('click', 'pins', (e) => {
        const id = e.features?.[0]?.properties?.id as string | undefined
        if (id) useStore.getState().selectPin(id)
        e.preventDefault()
      })
      m.on('click', (e) => {
        if (!e.defaultPrevented) useStore.getState().selectPin(null)
      })
      m.on('mouseenter', 'pins', () => (m.getCanvas().style.cursor = 'pointer'))
      m.on('mouseleave', 'pins', () => (m.getCanvas().style.cursor = ''))
      m.on('contextmenu', (e) => contextMenuRef.current?.({ lngLat: e.lngLat, x: e.point.x, y: e.point.y }))
      m.on('dragstart', () => {
        if (useStore.getState().settings.followPlayer) useStore.getState().setSettings({ followPlayer: false })
      })
    }
    return () => {
      if (mapHandle.current === m) mapHandle.current = null
      setMap(null)
      m.remove()
    }
  }, [game, gameMap, variant, hasSize])

  useRefitOnResize(map, variant, containerRef, fitPadding)
  usePinsLayer(map)
  useLiveMarkers(map)
  useLayerShading(map)
  useTrackLine(map)
  usePlayers(map, variant)
  useMiniZoomKeys(map, variant)
  // MapLibre's CSS forces `position: relative` on its container, so size it from a wrapper.
  return (
    <div className="absolute inset-0">
      <div ref={containerRef} className="h-full w-full" />
    </div>
  )
}


/**
 * Keeps the island fitted when the screen changes (resolution, monitor, ultrawide,
 * panel or interface size). Pans and zooms the user made at the same size are kept.
 */
function useRefitOnResize(map: MlMap | null, variant: 'full' | 'mini', containerRef: React.RefObject<HTMLDivElement | null>, padding: Insets) {
  const { top, right, bottom, left } = padding
  useEffect(() => {
    const el = containerRef.current
    const gameMap = useStore.getState().map
    if (!map || !el || !gameMap || variant !== 'full') return
    const pad = { top, right, bottom, left }
    let last = `${el.clientWidth}x${el.clientHeight}`
    fitIsland(map, gameMap.projection, pad)
    const ro = new ResizeObserver(() => {
      const size = `${el.clientWidth}x${el.clientHeight}`
      if (size === last || el.clientWidth === 0 || el.clientHeight === 0) return
      last = size
      map.resize()
      if (!useStore.getState().settings.followPlayer) fitIsland(map, gameMap.projection, pad)
    })
    ro.observe(el)
    return () => ro.disconnect()
  }, [map, variant, containerRef, top, right, bottom, left])
}

function usePinsLayer(map: MlMap | null) {
  const game = useStore((s) => s.game)
  const gameMap = useStore((s) => s.map)
  const gamePins = useStore((s) => s.gamePins)
  const user = useStore((s) => s.user)
  const selected = useStore((s) => s.selectedPinId)
  const showFound = useStore((s) => s.showFound)
  const viewLayer = useStore(selectViewLayer)
  const pins = useMemo(() => selectAllPins({ gamePins, user }), [gamePins, user])

  useEffect(() => {
    if (!map || !game || !gameMap) return
    let cancelled = false
    const categories = new Map(game.categories.map((c) => [c.id, c]))
    const found = new Set(user.found)
    const visible: { pin: Pin; key: string; color: string; icon: string }[] = []
    for (const pin of pins) {
      const cat = categories.get(pin.category)
      if (!isCategoryVisible(cat, user) && pin.id !== selected) continue
      if (!showFound && found.has(pin.id) && pin.id !== selected) continue
      const color = pin.color ?? cat?.color ?? '#ffffff'
      const icon = pin.icon ?? cat?.icon ?? 'builtin:pin'
      visible.push({ pin, key: badgeKey(color, icon), color, icon })
    }
    const needed = new Map(visible.map((v) => [v.key, v]))
    void Promise.all([...needed.values()].map((v) => ensureBadge(map, v.key, game.id, v.color, v.icon))).then(() => {
      if (cancelled) return
      const source = map.getSource<GeoJSONSource>('pins')
      source?.setData({
        type: 'FeatureCollection',
        features: visible.map(({ pin, key }) => {
          const ll = toLngLat(gameMap.projection, pin.position)
          return {
            type: 'Feature',
            geometry: { type: 'Point', coordinates: [ll.lng, ll.lat] },
            properties: {
              id: pin.id,
              icon: key,
              name: pin.name,
              found: found.has(pin.id),
              selected: pin.id === selected,
              otherLayer: (pin.layer ?? 'surface') !== viewLayer && pin.id !== selected,
            },
          }
        }),
      })
    })
    return () => {
      cancelled = true
    }
  }, [map, game, gameMap, pins, user, selected, showFound, viewLayer])
}

/** Color of live GPS marker badges, distinct from every pin category. */
const LIVE_MARKER_COLOR = '#475569'

/** Draws the game's live GPS markers that the static pins don't already cover. */
function useLiveMarkers(map: MlMap | null) {
  const game = useStore((s) => s.game)
  const gameMap = useStore((s) => s.map)
  const gamePins = useStore((s) => s.gamePins)
  const filter = useMemo(() => (game ? new LiveMarkerFilter(gamePins, game.unitsPerMeter) : null), [gamePins, game])

  useEffect(() => {
    if (!map || !game || !gameMap || !filter) return
    let last = ''
    let cancelled = false
    const apply = (markers: Marker[]) => {
      const visible = filter.visible(markers)
      const key = visible.map((m) => `${m.id}:${m.icon}:${m.position.x.toFixed(0)},${m.position.z.toFixed(0)}`).join('|')
      if (key === last) return
      last = key
      const icons = new Set(visible.map((m) => `game:${m.icon}`))
      void Promise.all([...icons].map((icon) => ensureBadge(map, badgeKey(LIVE_MARKER_COLOR, icon), game.id, LIVE_MARKER_COLOR, icon))).then(() => {
        if (cancelled) return
        map.getSource<GeoJSONSource>('markers')?.setData({
          type: 'FeatureCollection',
          features: visible.map((m) => {
            const ll = toLngLat(gameMap.projection, m.position)
            return {
              type: 'Feature',
              geometry: { type: 'Point', coordinates: [ll.lng, ll.lat] },
              properties: { id: m.id, name: markerName(m), icon: badgeKey(LIVE_MARKER_COLOR, `game:${m.icon}`) },
            }
          }),
        })
      })
    }
    apply(useStore.getState().snapshot?.markers ?? [])
    const unsubscribe = useStore.subscribe((s, prev) => {
      if (s.snapshot !== prev.snapshot) apply(s.snapshot?.markers ?? [])
    })
    return () => {
      cancelled = true
      unsubscribe()
    }
  }, [map, game, gameMap, filter])
}

/** Underground view: dims the surface imagery and shows the cave layouts on top. */
function useLayerShading(map: MlMap | null) {
  const underground = useStore(selectViewLayer) === 'caves'
  useEffect(() => {
    if (!map) return
    map.setPaintProperty('base', 'raster-brightness-max', underground ? 0.32 : 1)
    map.setPaintProperty('base', 'raster-saturation', underground ? -0.7 : 0)
    if (map.getLayer(CAVES_LAYER)) map.setLayoutProperty(CAVES_LAYER, 'visibility', underground ? 'visible' : 'none')
    map.setPaintProperty('background', 'background-color', underground ? '#11161c' : (useStore.getState().map?.background ?? '#000'))
  }, [map, underground])
}

function useTrackLine(map: MlMap | null) {
  const gameMap = useStore((s) => s.map)
  const tracked = useStore((s) => s.tracked)
  const snapshot = useStore((s) => s.snapshot)
  const gamePins = useStore((s) => s.gamePins)
  const user = useStore((s) => s.user)

  useEffect(() => {
    if (!map || !gameMap) return
    const source = map.getSource<GeoJSONSource>('track')
    const from = snapshot?.local?.position
    let to = null
    if (tracked?.kind === 'pin') to = selectAllPins({ gamePins, user }).find((p) => p.id === tracked.id)?.position
    if (tracked?.kind === 'player') to = findMover(snapshot, tracked.id)?.position
    if (!from || !to) {
      source?.setData({ type: 'FeatureCollection', features: [] })
      return
    }
    const a = toLngLat(gameMap.projection, from)
    const b = toLngLat(gameMap.projection, to)
    source?.setData({
      type: 'Feature',
      geometry: { type: 'LineString', coordinates: [[a.lng, a.lat], [b.lng, b.lat]] },
      properties: {},
    })
  }, [map, gameMap, tracked, snapshot, gamePins, user])
}

function usePlayers(map: MlMap | null, variant: 'full' | 'mini') {
  const gameMap = useStore((s) => s.map)
  const markers = useRef<PlayerMarkers | null>(null)

  useEffect(() => {
    if (!map || !gameMap) return
    const pm = new PlayerMarkers(map, gameMap.projection, variant === 'mini')
    markers.current = pm
    const apply = () => {
      const { snapshot, settings } = useStore.getState()
      pm.update(snapshot)
      const local = snapshot?.local
      if (variant === 'mini') {
        const zoom = miniBaseZoom(gameMap.projection) + settings.hud.zoom
        if (!local) return map.jumpTo({ zoom })
        const center = toLngLat(gameMap.projection, local.position)
        const heading = local.forward ? bearingOf(gameMap.projection, local.forward) : null
        map.jumpTo({
          center: [center.lng, center.lat],
          zoom,
          bearing: settings.hud.rotateWithPlayer && heading !== null ? heading : 0,
        })
        return
      }
      if (!local) return
      const center = toLngLat(gameMap.projection, local.position)
      if (settings.followPlayer) {
        map.jumpTo({ center: [center.lng, center.lat] })
      }
    }
    apply()
    const unsubscribe = useStore.subscribe((s, prev) => {
      if (s.snapshot !== prev.snapshot || s.settings !== prev.settings) apply()
    })
    return () => {
      unsubscribe()
      pm.remove()
      markers.current = null
    }
  }, [map, gameMap, variant])
}

/** Zoom keys pressed in the game zoom the mini-map; the zoom is saved with the settings. */
function useMiniZoomKeys(map: MlMap | null, variant: 'full' | 'mini') {
  const gameMap = useStore((s) => s.map)
  useEffect(() => {
    if (!map || !gameMap || variant !== 'mini') return
    const unlisten = events.mapZoom((steps) => {
      const { settings, setHud } = useStore.getState()
      const base = miniBaseZoom(gameMap.projection)
      const zoom = steppedZoom(base + settings.hud.zoom, steps, map.getMinZoom(), map.getMaxZoom())
      setHud({ zoom: zoom - base })
    })
    return () => void unlisten.then((f) => f())
  }, [map, gameMap, variant])
}
