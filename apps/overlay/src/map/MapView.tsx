import { Map as MlMap, type GeoJSONSource, type ImageSource } from 'maplibre-gl'
import 'maplibre-gl/dist/maplibre-gl.css'
import './setup'
import { useEffect, useMemo, useRef, useState } from 'react'
import { events } from '../lib/backend'
import { GAME_MARKERS_CATEGORY, type Pin } from '../lib/game'
import { LiveMarkerFilter, markerName } from '../lib/markers'
import { findMover } from '../lib/movers'
import { resolveTracked } from '../lib/tracking'
import { bearingOf, mapBounds, mapCenter, toLngLat, zoomToFit, type LngLat, type Projection } from '../lib/projection'
import { CREATURE_COLORS, creatureName, visibleCreatures } from '../lib/creatures'
import { PING_MS, useSyncStore, type Ping } from '../lib/sync/sync'
import type { Creature, Marker, Vec3 } from '../lib/protocol'
import { fogPixels } from '../lib/fog'
import { useFogStore, usePinFog } from '../state/fog'
import { steppedZoom } from '../lib/zoom'
import { isCategoryVisible, selectAllPins, selectViewLayer, useStore } from '../state/store'
import { badgeKey, ensureBadge } from './badges'
import { mapHandle } from './handle'
import { PlayerMarkers } from './players'
import { applyPinDisplay, buildStyle, CAVES_LAYER } from './style'
import { useGameSetting, useSettings } from '../settings/store'

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
      zoom: miniBaseZoom(p) + (full ? 0 : useSettings.getState().ui.hud.zoom),
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
      for (const layer of ['pins', 'pin-dots']) {
        m.on('click', layer, (e) => {
          if (e.defaultPrevented) return // a badge on top already took the click
          const id = e.features?.[0]?.properties?.id as string | undefined
          if (id) useStore.getState().selectPin(id)
          e.preventDefault()
        })
        m.on('mouseenter', layer, () => (m.getCanvas().style.cursor = 'pointer'))
        m.on('mouseleave', layer, () => (m.getCanvas().style.cursor = ''))
      }
      m.on('click', 'markers', (e) => {
        if (e.defaultPrevented) return
        const id = e.features?.[0]?.properties?.id as string | undefined
        if (id) useStore.getState().selectLive(id)
        e.preventDefault()
      })
      m.on('mouseenter', 'markers', () => (m.getCanvas().style.cursor = 'pointer'))
      m.on('mouseleave', 'markers', () => (m.getCanvas().style.cursor = ''))
      m.on('click', (e) => {
        // Player and companion markers are HTML elements and handle their own clicks.
        if (e.defaultPrevented || (e.originalEvent.target as HTMLElement | null)?.closest('.player-remote')) return
        useStore.getState().selectPin(null)
        useStore.getState().selectLive(null)
      })
      m.on('contextmenu', (e) => contextMenuRef.current?.({ lngLat: e.lngLat, x: e.point.x, y: e.point.y }))
      m.on('dragstart', () => {
        const s = useStore.getState()
        if (s.followPlayer) s.setFollowPlayer(false)
        if (s.followId) s.setFollow(null)
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
  usePinDisplay(map)
  useLiveMarkers(map)
  useCreatures(map)
  useFogLayer(map)
  usePings(map)
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
 * Full map view: reopens where it was left (position and zoom, saved per game), or
 * fitted to the island the first time. Resizing keeps the center instead of jumping.
 */
function useRefitOnResize(map: MlMap | null, variant: 'full' | 'mini', containerRef: React.RefObject<HTMLDivElement | null>, padding: Insets) {
  const { top, right, bottom, left } = padding
  useEffect(() => {
    const el = containerRef.current
    const gameMap = useStore.getState().map
    if (!map || !el || !gameMap || variant !== 'full') return
    const pad = { top, right, bottom, left }
    let last = `${el.clientWidth}x${el.clientHeight}`
    const saved = useStore.getState().mapView
    if (saved) map.jumpTo(saved)
    else fitIsland(map, gameMap.projection, pad)
    const save = () => useStore.getState().setMapView({ center: map.getCenter().toArray() as [number, number], zoom: map.getZoom() })
    map.on('moveend', save)
    const ro = new ResizeObserver(() => {
      const size = `${el.clientWidth}x${el.clientHeight}`
      if (size === last || el.clientWidth === 0 || el.clientHeight === 0) return
      last = size
      map.resize() // keeps the center
    })
    ro.observe(el)
    return () => {
      ro.disconnect()
      map.off('moveend', save)
    }
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
  const friendPins = useStore((s) => s.friendPins)
  const pins = useMemo(() => selectAllPins({ gamePins, user, friendPins }), [gamePins, user, friendPins])
  const hiddenByFog = usePinFog()

  useEffect(() => {
    if (!map || !game || !gameMap) return
    let cancelled = false
    const categories = new Map(game.categories.map((c) => [c.id, c]))
    const rank = new Map(game.categories.map((c, i) => [c.id, i]))
    const found = new Set(user.found)
    const visible: { pin: Pin; key: string; color: string; icon: string }[] = []
    for (const pin of pins) {
      const cat = categories.get(pin.category)
      if (!isCategoryVisible(cat, user) && pin.id !== selected) continue
      if (!showFound && found.has(pin.id) && pin.id !== selected) continue
      if (hiddenByFog?.(pin)) continue
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
        features: visible.map(({ pin, key, color }) => {
          const ll = toLngLat(gameMap.projection, pin.position)
          return {
            type: 'Feature',
            geometry: { type: 'Point', coordinates: [ll.lng, ll.lat] },
            properties: {
              id: pin.id,
              icon: key,
              color,
              rank: rank.get(pin.category) ?? game.categories.length,
              name: pin.name,
              found: found.has(pin.id),
              selected: pin.id === selected,
              // Entrances connect both layers, so they stay fully visible on either.
              otherLayer: (pin.layer ?? 'surface') !== viewLayer && pin.id !== selected && !categories.get(pin.category)?.entrance,
            },
          }
        }),
      })
    })
    return () => {
      cancelled = true
    }
  }, [map, game, gameMap, pins, user, selected, showFound, viewLayer, hiddenByFog])
}

/** Applies the user's pin size and declutter settings. */
function usePinDisplay(map: MlMap | null) {
  const gameMap = useStore((s) => s.map)
  const size = useSettings((s) => s.ui.pinSize)
  const declutter = useSettings((s) => s.ui.declutter)
  useEffect(() => {
    if (map && gameMap) applyPinDisplay(map, gameMap, { size, declutter })
  }, [map, gameMap, size, declutter])
}

/** Color of live GPS marker badges, distinct from every pin category. */
const LIVE_MARKER_COLOR = '#475569'

/** Draws the game's live GPS markers that the static pins don't already cover. */
function useLiveMarkers(map: MlMap | null) {
  const game = useStore((s) => s.game)
  const gameMap = useStore((s) => s.map)
  const gamePins = useStore((s) => s.gamePins)
  const filter = useMemo(() => (game ? new LiveMarkerFilter(gamePins, game.unitsPerMeter) : null), [gamePins, game])
  const shown = useStore((s) => isCategoryVisible(s.game?.categories.find((c) => c.id === GAME_MARKERS_CATEGORY), s.user))

  useEffect(() => {
    if (!map || !game || !gameMap || !filter) return
    // Not a possible key, so the first pass always draws (hiding markers must clear them).
    let last: string | null = null
    let cancelled = false
    const apply = (markers: Marker[]) => {
      const visible = shown ? filter.visible(markers) : []
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
  }, [map, game, gameMap, filter, shown])
}

/** Creatures around the player, refreshed with each snapshot. */
function useCreatures(map: MlMap | null) {
  const gameMap = useStore((s) => s.map)
  const gameId = useStore((s) => s.game?.id)
  const show = useGameSetting(gameId, 'showCreatures')
  const animals = useGameSetting(gameId, 'showAnimals')
  useEffect(() => {
    if (!map || !gameMap) return
    let last: Creature[] | undefined
    const apply = (creatures: Creature[] = []) => {
      if (creatures === last) return
      last = creatures
      map.getSource<GeoJSONSource>('creatures')?.setData({
        type: 'FeatureCollection',
        features: visibleCreatures(creatures, show, animals).map((c) => {
          const ll = toLngLat(gameMap.projection, c.position)
          return {
            type: 'Feature',
            geometry: { type: 'Point', coordinates: [ll.lng, ll.lat] },
            properties: { id: c.id, name: creatureName(c.name), color: CREATURE_COLORS[c.kind] },
          }
        }),
      })
    }
    apply(useStore.getState().snapshot?.creatures)
    return useStore.subscribe((s, prev) => {
      if (s.snapshot !== prev.snapshot) apply(s.snapshot?.creatures)
    })
  }, [map, gameMap, show, animals])
}

/** Spots players pinged, fading out over `PING_MS`. */
function usePings(map: MlMap | null) {
  const gameMap = useStore((s) => s.map)
  useEffect(() => {
    if (!map || !gameMap) return
    const apply = (pings: Ping[]) => {
      const now = Date.now()
      map.getSource<GeoJSONSource>('pings')?.setData({
        type: 'FeatureCollection',
        features: pings.map((p) => {
          const ll = toLngLat(gameMap.projection, p.position)
          return {
            type: 'Feature',
            geometry: { type: 'Point', coordinates: [ll.lng, ll.lat] },
            properties: { label: p.from ?? 'You', fade: Math.max(0.15, 1 - (now - p.at) / PING_MS) },
          }
        }),
      })
    }
    apply(useSyncStore.getState().pings)
    const unsubscribe = useSyncStore.subscribe((s) => apply(s.pings))
    // Pings fade with age: redraw while any are up.
    const timer = setInterval(() => {
      const { pings } = useSyncStore.getState()
      if (pings.length) apply(pings)
    }, 1000)
    return () => {
      unsubscribe()
      clearInterval(timer)
    }
  }, [map, gameMap])
}

const FOG_SOURCE = 'fog'
const FOG_LAYER = 'fog'

/** Most often the fog image is redrawn while exploring. */
const FOG_REDRAW_MS = 1500
const FOG_COLOR: [number, number, number] = [14, 18, 22]
/** Fog image pixels per fog cell (for soft edges). */
const FOG_SCALE = 2
/** Opacity of the fog when it only shades unexplored areas. */
const FOG_SHADE = 0.55

/** Fog of war over the surface imagery (see `state/fog`). */
function useFogLayer(map: MlMap | null) {
  const gameMap = useStore((s) => s.map)
  const enabled = useSettings((s) => s.ui.fogOfWar)
  const hideMap = useSettings((s) => s.ui.fogHideMap)
  const surface = useStore(selectViewLayer) === 'surface'
  const fog = useFogStore((s) => s.fog)
  const version = useFogStore((s) => s.version)
  const drawnAt = useRef(0)
  const pending = useRef<ReturnType<typeof setTimeout> | null>(null)

  useEffect(() => {
    if (!map || !gameMap || !fog) return
    const draw = () => {
      pending.current = null
      drawnAt.current = Date.now()
      const cells = document.createElement('canvas')
      cells.width = fog.width
      cells.height = fog.height
      cells.getContext('2d')?.putImageData(new ImageData(fogPixels(fog, FOG_COLOR), fog.width, fog.height), 0, 0)
      // Upscaled and blurred, so explored areas have soft edges rather than 8 m steps.
      const canvas = document.createElement('canvas')
      canvas.width = fog.width * FOG_SCALE
      canvas.height = fog.height * FOG_SCALE
      const ctx = canvas.getContext('2d')
      if (ctx) {
        ctx.filter = `blur(${FOG_SCALE * 1.5}px)`
        ctx.drawImage(cells, 0, 0, canvas.width, canvas.height)
      }
      const p = gameMap.projection
      const corner = (east: number, north: number) => {
        const v: Vec3 = { x: 0, y: 0, z: 0 }
        v[p.east] = east
        v[p.north] = north
        const ll = toLngLat(p, v)
        return [ll.lng, ll.lat] as [number, number]
      }
      const [w, s, e, n] = [fog.minEast, fog.minNorth, fog.minEast + fog.width * fog.cell, fog.minNorth + fog.height * fog.cell]
      const coordinates: [[number, number], [number, number], [number, number], [number, number]] = [corner(w, n), corner(e, n), corner(e, s), corner(w, s)]
      const url = canvas.toDataURL()
      const source = map.getSource<ImageSource>(FOG_SOURCE)
      if (source) {
        source.updateImage({ url, coordinates })
      } else {
        map.addSource(FOG_SOURCE, { type: 'image', url, coordinates })
        map.addLayer(
          { id: FOG_LAYER, type: 'raster', source: FOG_SOURCE, paint: { 'raster-opacity': FOG_SHADE, 'raster-fade-duration': 0, 'raster-resampling': 'linear' } },
          // Above the imagery and cave layouts (hidden on the surface), under routes and pins.
          'track-line',
        )
      }
    }
    // Exploring changes the fog every few steps: redraw at most every FOG_REDRAW_MS.
    const wait = FOG_REDRAW_MS - (Date.now() - drawnAt.current)
    if (wait <= 0) draw()
    else pending.current ??= setTimeout(draw, wait)
  }, [map, gameMap, fog, version])

  useEffect(() => () => void (pending.current && clearTimeout(pending.current)), [])

  useEffect(() => {
    if (!map?.getLayer(FOG_LAYER)) return
    map.setLayoutProperty(FOG_LAYER, 'visibility', enabled && surface ? 'visible' : 'none')
    // Covering hides what hasn't been explored; shading only marks it.
    map.setPaintProperty(FOG_LAYER, 'raster-opacity', hideMap ? 1 : FOG_SHADE)
  }, [map, enabled, hideMap, surface, fog, version])
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
  const route = useStore((s) => s.route)

  useEffect(() => {
    if (!map || !gameMap) return
    const source = map.getSource<GeoJSONSource>('track')
    const from = snapshot?.local?.position
    const to = tracked ? resolveTracked(tracked, snapshot).position : undefined
    if (!from || !to) {
      source?.setData({ type: 'FeatureCollection', features: [] })
      return
    }
    // The walking route when there is one, else a straight line.
    const points = route && route.corners.length >= 2 ? [from, ...route.corners.slice(1), to] : [from, to]
    source?.setData({
      type: 'Feature',
      geometry: {
        type: 'LineString',
        coordinates: points.map((p) => {
          const ll = toLngLat(gameMap.projection, p)
          return [ll.lng, ll.lat]
        }),
      },
      properties: {},
    })
  }, [map, gameMap, tracked, snapshot, gamePins, user, route])
}

function usePlayers(map: MlMap | null, variant: 'full' | 'mini') {
  const gameMap = useStore((s) => s.map)
  const markers = useRef<PlayerMarkers | null>(null)

  useEffect(() => {
    if (!map || !gameMap) return
    const pm = new PlayerMarkers(map, gameMap.projection, variant === 'mini', variant === 'full' ? (id) => useStore.getState().selectLive(id) : undefined)
    markers.current = pm
    const apply = () => {
      const { snapshot, followPlayer } = useStore.getState()
      const { hud } = useSettings.getState().ui
      pm.update(snapshot)
      const local = snapshot?.local
      if (variant === 'mini') {
        const zoom = miniBaseZoom(gameMap.projection) + hud.zoom
        if (!local) return map.jumpTo({ zoom })
        const center = toLngLat(gameMap.projection, local.position)
        const heading = local.forward ? bearingOf(gameMap.projection, local.forward) : null
        map.jumpTo({
          center: [center.lng, center.lat],
          zoom,
          bearing: hud.rotateWithPlayer && heading !== null ? heading : 0,
        })
        return
      }
      const followed = useStore.getState().followId
      const target = followed ? findMover(snapshot, followed)?.position : followPlayer ? local?.position : undefined
      if (target) {
        const center = toLngLat(gameMap.projection, target)
        map.jumpTo({ center: [center.lng, center.lat] })
      }
    }
    apply()
    const unsubscribe = useStore.subscribe((s, prev) => {
      if (s.snapshot !== prev.snapshot || s.followPlayer !== prev.followPlayer || s.followId !== prev.followId) apply()
    })
    const unsubscribeHud = useSettings.subscribe((s, prev) => {
      if (s.ui.hud !== prev.ui.hud) apply()
    })
    return () => {
      unsubscribe()
      unsubscribeHud()
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
      const { ui, setHud } = useSettings.getState()
      const base = miniBaseZoom(gameMap.projection)
      const zoom = steppedZoom(base + ui.hud.zoom, steps, map.getMinZoom(), map.getMaxZoom())
      setHud({ zoom: zoom - base })
    })
    return () => void unlisten.then((f) => f())
  }, [map, gameMap, variant])
}
