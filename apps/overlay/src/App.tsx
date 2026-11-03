import { Component, useEffect, useState, type ReactNode } from 'react'
import { pinsReached } from './lib/autoFound'
import { resolveTracked } from './lib/tracking'
import { findPin, refreshBackSteps } from './lib/actions'
import { caveAt, caveIndex, caveRoute } from './lib/caveNav'
import { nearestEntrance, routeDestination } from './lib/route'
import type { Vec3 } from './lib/protocol'
import { startAgentLogBuffer } from './lib/agentLogs'
import { backend, events } from './lib/backend'
import { loadGame } from './lib/game'
import { matchesHotkey } from './lib/keys'
import { steppedZoom } from './lib/zoom'
import { mapHandle } from './map/handle'
import { useSync } from './lib/sync/sync'
import { loadSession, loadUserData, startPersisting } from './state/persist'
import { selectAllPins, useStore } from './state/store'
import type { Hotkey } from './lib/protocol'
import { FullMap } from './ui/FullMap'
import { HUD_MARGIN, hudSize } from './lib/hudLayout'
import { Hud } from './ui/Hud'
import { gameSettings } from './settings/model'
import { mapHotkey, useSettings } from './settings/store'
import { startSettings } from './settings/sync'

/** A new map pack arrived while the full map was open. */
let reloadWhenClosed = false

function useBackendSync() {
  useEffect(() => {
    const { setStatus, setMode, setSnapshot, notify } = useStore.getState()
    const cleanups: Promise<() => void>[] = [
      startSettings(),
      events.status(setStatus),
      events.teleportBack((error) => {
        void refreshBackSteps()
        if (useStore.getState().mode !== 'hidden') notify(error ? `Teleport back failed: ${error}` : 'Teleported back', error ? 'error' : 'info')
      }),
      events.snapshot((s) => setSnapshot(s.local || s.others.length ? s : null)),
      events.overlayMode(async (mode) => {
        setMode(mode)
        if (mode !== 'open' && reloadWhenClosed) return window.location.reload()
        // Snapshots are only streamed while visible; catch up immediately.
        if (mode !== 'hidden') setSnapshot(await backend.lastSnapshot())
        if (mode === 'open') void refreshBackSteps()
        if (mode === 'open') requestAnimationFrame(() => mapHandle.current?.resize())
      }),
      startAgentLogBuffer(),
      // New map data for the shown game: reload so tiles, icons and pins all switch
      // over, but not while the user is looking at the map.
      events.mapPackInstalled((gameId) => {
        if (useStore.getState().game?.id !== gameId) return
        if (useStore.getState().mode === 'open') reloadWhenClosed = true
        else window.location.reload()
      }),
    ]
    const stopPersist = startPersisting()
    void (async () => {
      setStatus(await backend.status())
      setMode(await backend.overlayMode())
      setSnapshot(await backend.lastSnapshot())
    })()
    return () => {
      stopPersist()
      cleanups.forEach((p) => void p.then((f) => f()))
    }
  }, [])
}

/** How often the route to the tracked target is recomputed. */
const ROUTE_INTERVAL_MS = 1500

/**
 * Keeps the walking route to the tracked target current while it's shown (map open,
 * or waypoint in the game). One navigation query in the game every 1.5 s.
 */
function useRoute() {
  const tracked = useStore((s) => s.tracked)
  const enabled = useSettings((s) => s.ui.showRoute)
  const waypoint = useSettings((s) => s.ui.hud.waypoint)
  const shown = useStore((s) => s.mode === 'open' || (s.mode === 'pinned' && waypoint))
  const canRoute = useStore((s) => s.status?.state === 'connected')
  useEffect(() => {
    const { setRoute } = useStore.getState()
    if (!tracked || !enabled || !shown || !canRoute) {
      setRoute(null)
      return
    }
    let cancelled = false
    const update = async () => {
      const s = useStore.getState()
      let target = resolveTracked(tracked, s.snapshot).position
      const local = s.snapshot?.local
      const game = s.game
      if (!target || !local || !game) return
      // Underground: route on the cave's floor grid (the game's pathfinding doesn't
      // cover caves), to the target if it's in this cave, else to the way out.
      if (local.layer === 'caves') {
        const cave = caveAt(await caveIndex(game.id), local.position)
        if (cave) {
          const inCave = (p: Vec3) => caveAt([cave], p, 8) !== undefined
          const dest = inCave(target)
            ? target
            : nearestEntrance(local.position, selectAllPins(s), game.categories, (pin) => inCave(pin.position))?.position
          const corners = dest ? await caveRoute(game.id, cave, local.position, dest, backend.checkRoute) : null
          if (cancelled) return
          if (corners) {
            setRoute({ corners, complete: true })
            return
          }
        }
      }
      if (tracked.kind === 'pin') {
        const pin = findPin(tracked.id)
        if (pin) target = routeDestination(pin, local.layer === 'caves', selectAllPins(s), game.categories)
      }
      try {
        const route = await backend.findPath(target)
        if (!cancelled) setRoute(route)
      } catch {
        if (!cancelled) setRoute(null) // no navigation mesh here, or no way there
      }
    }
    // Rerouting around blocks can outlast the interval: skip ticks while one runs.
    let running = false
    const tick = async () => {
      if (running) return
      running = true
      try {
        await update()
      } finally {
        running = false
      }
    }
    void tick()
    const id = setInterval(() => void tick(), ROUTE_INTERVAL_MS)
    return () => {
      cancelled = true
      clearInterval(id)
    }
  }, [tracked, enabled, shown, canRoute])
}

/** Marks collectibles found as the player passes them (see `lib/autoFound`). */
function useAutoFound() {
  useEffect(
    () =>
      useStore.subscribe((s, prev) => {
        const local = s.snapshot?.local
        if (s.snapshot === prev.snapshot || !local || !s.game || !s.map || !gameSettings(useSettings.getState().ui, s.game.id).autoFound) return
        const reached = pinsReached(s.gamePins, s.game.categories, new Set(s.user.found), local, s.map.projection, s.game.unitsPerMeter)
        if (reached.length === 0) return
        s.markFound(reached.map((p) => p.id))
        if (s.mode !== 'hidden') s.notify(`Found: ${reached.map((p) => p.name).join(', ')}`)
      }),
    [],
  )
}

/** Loads the detected game, or the first available one so the map can be browsed offline. */
function useGameLoader() {
  const detected = useStore((s) => s.status?.game_id ?? null)
  const current = useStore((s) => s.game?.id ?? null)
  useEffect(() => {
    let cancelled = false
    void (async () => {
      const id = detected ?? current ?? (await backend.listGames())[0]?.id
      if (!id || id === current) return
      try {
        const [{ game, pins }, user, session] = await Promise.all([loadGame(id), loadUserData(id), loadSession(id)])
        if (!cancelled) useStore.getState().setGame(game, pins, user, session)
      } catch (e) {
        useStore.getState().notify(`Could not load map: ${String(e)}`, 'error')
      }
    })()
    return () => {
      cancelled = true
    }
  }, [detected, current])
}

/** Keeps the backend's HUD window geometry in line with what the HUD shows. */
function useHudSync() {
  const hud = useSettings((s) => s.ui.hud)
  const tracking = useStore((s) => s.tracked !== null)
  useEffect(() => {
    const { width, height } = hudSize(hud.minimap, tracking, hud)
    void backend.setPinConfig({
      enabled: hud.minimap || tracking,
      corner: hud.corner,
      width,
      height,
      margin: HUD_MARGIN,
      fullscreen: tracking && hud.waypoint,
    })
  }, [hud, tracking])
}

/** Steps of a zoom key binding (+1 in, -1 out), or 0 if `e` isn't one. */
function zoomKeySteps(e: KeyboardEvent): number {
  const keys = useSettings.getState().app?.zoom_keys
  if (!keys) return 0
  const matches = (slots: (Hotkey | null)[]) => slots.some((h) => h && matchesHotkey(e, h))
  return matches(keys.zoom_in) ? 1 : matches(keys.zoom_out) ? -1 : 0
}

/**
 * While the full map has focus: M (or the configured key) and Esc close it, and the
 * zoom keys zoom it (the agent only sees keys while the game has focus).
 */
function useOverlayKeys() {
  const mode = useStore((s) => s.mode)
  useEffect(() => {
    if (mode !== 'open') return
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement
      if (target.closest('input, textarea, select, [contenteditable]')) return
      if (e.key === '/' || (e.ctrlKey && e.key.toLowerCase() === 'f')) {
        e.preventDefault()
        document.getElementById('pin-search')?.focus()
        return
      }
      const hotkey = mapHotkey(useSettings.getState().app, useStore.getState().game)
      if (e.key === 'Escape' || (hotkey && matchesHotkey(e, hotkey))) {
        e.preventDefault()
        void backend.closeOverlay()
        return
      }
      const steps = zoomKeySteps(e)
      const map = mapHandle.current
      if (steps && map) {
        e.preventDefault()
        // Held keys repeat faster than an animation finishes; jump instead.
        map.easeTo({ zoom: steppedZoom(map.getZoom(), steps, map.getMinZoom(), map.getMaxZoom()), duration: e.repeat ? 0 : 120 })
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [mode])
}

export default function App() {
  useBackendSync()
  useAutoFound()
  useRoute()
  useSync()
  useGameLoader()
  useHudSync()
  useOverlayKeys()
  const mode = useStore((s) => s.mode)
  const hasGame = useStore((s) => s.game !== null)
  // Mount the full map on first open, then keep it (laid out but invisible) so
  // reopening is instant. It must never be display:none: MapLibre needs a real size.
  const [fullMapMounted, setFullMapMounted] = useState(false)
  if (mode === 'open' && !fullMapMounted) setFullMapMounted(true)
  if (!hasGame) return null

  return (
    <ErrorBoundary>
      {fullMapMounted && (
        <div className={`absolute inset-0 ${mode === 'open' ? '' : 'invisible'}`} aria-hidden={mode !== 'open'}>
          <FullMap />
        </div>
      )}
      {mode === 'pinned' && (
        <div className="absolute inset-0">
          <Hud />
        </div>
      )}
    </ErrorBoundary>
  )
}

class ErrorBoundary extends Component<{ children: ReactNode }, { error: Error | null }> {
  state = { error: null as Error | null }

  static getDerivedStateFromError(error: Error) {
    return { error }
  }

  render() {
    if (!this.state.error) return this.props.children
    return (
      <div className="grid h-full place-items-center">
        <div className="glass max-w-md rounded-2xl p-5 text-[13px]">
          <h2 className="font-display mb-2 text-[16px] font-semibold">The map hit an error</h2>
          <p className="mb-3 break-words font-mono text-[12px] text-ink-dim">{this.state.error.message}</p>
          <button className="rounded-lg bg-accent px-3 py-1.5 font-semibold text-accent-ink" onClick={() => this.setState({ error: null })}>
            Try again
          </button>
        </div>
      </div>
    )
  }
}
