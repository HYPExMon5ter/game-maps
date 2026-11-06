import { useEffect, useState } from 'react'
import { pinsReached } from './lib/autoFound'
import { resolveTracked } from './lib/tracking'
import { bagMoved, clearDeathPin, findPin, markBag, markDeath, quickMark, refreshBackSteps, trackNearestUnfound } from './lib/actions'
import { caveAt, caveIndex, caveRoute } from './lib/caveNav'
import { nearestEntrance, routeDestination } from './lib/route'
import type { LocalPlayer, Vec3 } from './lib/protocol'
import { dueTimers } from './lib/timers'
import { startAgentLogBuffer } from './lib/agentLogs'
import { backend, events } from './lib/backend'
import { logUiError } from './lib/errors'
import { loadGame } from './lib/game'
import { matchesHotkey } from './lib/keys'
import { steppedZoom } from './lib/zoom'
import { mapHandle } from './map/handle'
import { acceptLatestRequest, announce, useSync, useSyncMirror } from './lib/sync/sync'
import { startPersisting } from './state/persist'
import { useTrail } from './state/trail'
import { loadWorldView, useWorldTracking } from './state/useWorlds'
import { useGamePins, useWorldExtras, useWorldMaps } from './state/worldMaps'
import { useFog } from './state/fog'
import { selectAllPins, useStore } from './state/store'
import type { Hotkey } from './lib/protocol'
import { FullMap } from './ui/FullMap'
import { HUD_MARGIN, hudSize } from './lib/hudLayout'
import { Hud } from './ui/Hud'
import { useOverGame } from './lib/overGame'
import { gameSettings } from './settings/model'
import { mapHotkey, useGameHud, useSettings } from './settings/store'
import { startSettings } from './settings/sync'
import { IS_MAP_WINDOW } from './lib/view'
import { ErrorBoundary } from './ui/ErrorBoundary'

/** A new map pack arrived while the full map was open. */
let reloadWhenClosed = false

/** `windowed`: the map window, which always shows the full map (see `lib/view`). */
function useBackendSync(windowed: boolean) {
  useEffect(() => {
    const { setStatus, setMode, setSnapshot, notify } = useStore.getState()
    const cleanups: Promise<() => void>[] = [
      startSettings(),
      events.status(setStatus),
      events.teleportBack((error) => {
        void refreshBackSteps()
        // Pressed in the game, so the map's toasts wouldn't show.
        if (useStore.getState().mode === 'pinned') announce(error ? `Teleport back failed: ${error}` : 'Teleported back', error ? 'failed' : 'done')
        else if (useStore.getState().mode === 'open') notify(error ? `Teleport back failed: ${error}` : 'Teleported back', error ? 'error' : 'info')
      }),
      events.snapshot((s) => setSnapshot(s.local || s.others.length ? s : null)),
      events.snapshotUpdate((u) => {
        const { snapshot } = useStore.getState()
        // Lists stay the same arrays: whatever draws them can tell nothing changed.
        if (snapshot) setSnapshot(u.local || u.others.length ? { ...snapshot, companions: undefined, environment: undefined, dropped_bag: undefined, dying: false, indoors: false, progress: undefined, ...u } : null)
        else if (u.local) void backend.lastSnapshot().then(setSnapshot)
      }),
      events.overlayMode(async (mode) => {
        if (windowed) return
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
        if (useStore.getState().mode === 'open' && !windowed) reloadWhenClosed = true
        else window.location.reload()
      }),
    ]
    const stopPersist = startPersisting()
    void (async () => {
      setStatus(await backend.status())
      setMode(windowed ? 'open' : await backend.overlayMode())
      setSnapshot(await backend.lastSnapshot())
    })()
    return () => {
      stopPersist()
      cleanups.forEach((p) => void p.then((f) => f()))
    }
  }, [windowed])
}

/** How often the route to the tracked target is recomputed. */
const ROUTE_INTERVAL_MS = 1500
/** Each route is a blocking query on the game's main thread, so standing still skips it... */
const ROUTE_MOVED_METERS = 5
/** ...for this long at most: the world can change around a player who isn't moving. */
const ROUTE_MAX_AGE_MS = 15_000

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
    let asked: { from: Vec3; to: Vec3; at: number } | null = null
    const update = async () => {
      const s = useStore.getState()
      let target = resolveTracked(tracked, s.snapshot).position
      const local = s.snapshot?.local
      const game = s.game
      if (!target || !local || !game) return
      const moved = (a: Vec3, b: Vec3) => Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z) >= ROUTE_MOVED_METERS * game.unitsPerMeter
      if (s.route && asked && Date.now() - asked.at < ROUTE_MAX_AGE_MS && !moved(asked.from, local.position) && !moved(asked.to, target)) return
      asked = { from: local.position, to: target, at: Date.now() }
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
        // The game is in a world the map hasn't switched to yet: this isn't a find in this one.
        if (s.snapshot?.world && s.snapshot.world.id !== s.world.id) return
        const reached = pinsReached(s.gamePins, s.game.categories, new Set(s.user.found), local, s.map.projection, s.game.unitsPerMeter)
        if (reached.length === 0) return
        s.markFound(reached.map((p) => p.id))
        if (s.mode !== 'hidden') s.notify(`Found: ${reached.map((p) => p.name).join(', ')}`)
      }),
    [],
  )
}

/** Keys for what the map does itself while the game has focus. */
function useActionKeys() {
  useEffect(() => {
    const unlisten = events.hotkeyAction((action) => {
      if (action === 'quick_mark') {
        const pin = quickMark()
        announce(pin ? `Marked: ${pin.name}` : 'Nowhere to mark yet', pin ? 'pin' : 'failed')
      } else if (action === 'nearest_unfound') {
        const pin = trackNearestUnfound()
        announce(pin ? `Tracking the nearest: ${pin.name}` : 'Nothing left to find nearby', pin ? 'tracking' : 'failed')
      } else if (action === 'accept_request' && !acceptLatestRequest()) {
        announce('No requests to accept', 'failed')
      }
    })
    return () => void unlisten.then((f) => f())
  }, [])
}

/** Drops a pin where the player dies. */
function useDeathMarker() {
  useEffect(() => {
    // Where the player last was alive: the position while dying can be a cutscene's.
    let alive: LocalPlayer | null = null
    return useStore.subscribe((s, prev) => {
      const snapshot = s.snapshot
      if (snapshot === prev.snapshot || !snapshot) return
      if (s.game && gameSettings(useSettings.getState().ui, s.game.id).deathMarker) {
        const bag = snapshot.dropped_bag
        // Better than where the player fell: it's where their things actually are.
        if (bag && bagMoved(prev.snapshot?.dropped_bag, bag)) {
          markBag(bag, alive?.layer)
          if (prev.snapshot?.local) announce('Marked where your bag is', 'bag')
        } else if (!bag && prev.snapshot?.dropped_bag && snapshot.local) {
          clearDeathPin()
          announce('Bag recovered', 'bag')
        }
      }
      if (!snapshot.dying) {
        alive = snapshot.local ?? alive
        return
      }
      if (prev.snapshot?.dying || !alive || !s.game || !gameSettings(useSettings.getState().ui, s.game.id).deathMarker) return
      markDeath(alive)
      announce('Marked where you died', 'death')
    })
  }, [])
}

/** How often pin timers are checked. */
const TIMER_CHECK_MS = 1000

/** Says when a pin's countdown is up. */
function usePinTimers() {
  useEffect(() => {
    const id = setInterval(() => {
      const s = useStore.getState()
      if (Object.keys(s.user.timers).length === 0) return
      const { due, running } = dueTimers(s.user.timers, Date.now())
      if (due.length === 0) return
      s.setPinTimers(running)
      for (const pinId of due) announce(`Time's up: ${findPin(pinId)?.name ?? 'a pin'}`, 'timer')
    }, TIMER_CHECK_MS)
    return () => clearInterval(id)
  }, [])
}

/** How long to wait before trying a failed map load again. */
const MAP_RETRY_MS = 5000

/** Loads the detected game, else the one asked for, else the first so the map can be browsed offline. */
function useGameLoader() {
  const detected = useStore((s) => s.status?.game_id ?? null)
  const current = useStore((s) => s.game?.id ?? null)
  // Bumped to try again after a failed load.
  const [attempt, setAttempt] = useState(0)
  const [requested, setRequested] = useState<string | null>(null)
  useEffect(() => {
    const unlisten = events.showGame((id) => {
      const { status, game } = useStore.getState()
      if (!status?.game_id && game?.id !== id) setRequested(id)
    })
    return () => void unlisten.then((stop) => stop())
  }, [])
  useEffect(() => {
    let cancelled = false
    let retry: ReturnType<typeof setTimeout> | undefined
    void (async () => {
      let id = detected ?? requested ?? current
      try {
        id ??= (await backend.listGames())[0]?.id
        if (!id || id === current) return
        const { game, pins } = await loadGame(id)
        const view = await loadWorldView(game)
        if (cancelled) return
        useStore.getState().setGame(game, view.pins ?? pins, view.user, view.world, view.index, view.session, !!view.pins)
        setRequested(null)
      } catch (e) {
        logUiError(`could not load map ${id}: ${String(e)}`)
        useStore.getState().notify(`Could not load map: ${String(e)}`, 'error')
        if (!cancelled) retry = setTimeout(() => setAttempt((a) => a + 1), MAP_RETRY_MS)
      }
    })()
    return () => {
      cancelled = true
      clearTimeout(retry)
    }
  }, [detected, requested, current, attempt])
}

/** Keeps the backend's HUD window geometry in line with what the HUD shows. */
function useHudSync() {
  const hud = useGameHud(useStore((s) => s.game?.id))
  const over = useOverGame()
  const minimap = hud.minimap && over.playing
  const tracking = useStore((s) => s.tracked !== null) && over.playing
  const enabled = minimap || tracking || over.waypoint || over.pings || over.danger || over.notice || over.friends
  useEffect(() => {
    const { width, height } = hudSize(minimap, tracking, hud)
    void backend.setPinConfig({
      enabled,
      anchor_x: hud.anchor.x,
      anchor_y: hud.anchor.y,
      width,
      height,
      margin: HUD_MARGIN,
      fullscreen: over.cover,
    })
  }, [hud, minimap, tracking, enabled, over.cover])
}

/** Notes that something was tracked. Every window's tracking reaches the overlay. */
function useTrackedCount() {
  const tracking = useStore((s) => s.tracked !== null)
  useEffect(() => {
    if (tracking) void backend.countFeature('tracked').catch(() => {})
  }, [tracking])
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
  // Around the windows' hooks too: without it, one error there ends the page for good,
  // and its listeners with it.
  return <ErrorBoundary>{IS_MAP_WINDOW ? <MapWindowApp /> : <OverlayApp />}</ErrorBoundary>
}

/** The map window: the full map only. The overlay does the rest (recording, multiplayer, HUD). */
function MapWindowApp() {
  useBackendSync(true)
  useRoute()
  useGameLoader()
  useWorldTracking(false)
  useFog(false)
  useWorldMaps(false)
  useSyncMirror()
  useTrail()
  const hasGame = useStore((s) => s.game !== null)
  if (!hasGame) return null
  return (
    <ErrorBoundary>
      <div className="absolute inset-0">
        <FullMap />
      </div>
    </ErrorBoundary>
  )
}

function OverlayApp() {
  useBackendSync(false)
  useAutoFound()
  useRoute()
  useSync()
  useActionKeys()
  useDeathMarker()
  usePinTimers()
  useTrail()
  useGameLoader()
  useWorldTracking(true)
  useFog(true)
  useWorldMaps(true)
  useGamePins()
  useWorldExtras()
  useHudSync()
  useTrackedCount()
  useOverlayKeys()
  const mode = useStore((s) => s.mode)
  const hasGame = useStore((s) => s.game !== null)
  // Mount the full map on first open, then keep it (laid out but invisible) so
  // reopening is instant. It must never be display:none: MapLibre needs a real size.
  const [fullMapMounted, setFullMapMounted] = useState(false)
  if (mode === 'open' && !fullMapMounted) setFullMapMounted(true)
  // The same for the HUD: rebuilding the mini-map would land just as play resumes.
  const [hudMounted, setHudMounted] = useState(false)
  if (mode === 'pinned' && !hudMounted) setHudMounted(true)
  if (!hasGame) return null

  return (
    <ErrorBoundary>
      {fullMapMounted && (
        <div className={`absolute inset-0 ${mode === 'open' ? '' : 'invisible'}`} aria-hidden={mode !== 'open'}>
          <FullMap />
        </div>
      )}
      {hudMounted && (
        <div className={`absolute inset-0 ${mode === 'pinned' ? '' : 'invisible'}`} aria-hidden={mode !== 'pinned'}>
          <Hud />
        </div>
      )}
    </ErrorBoundary>
  )
}
