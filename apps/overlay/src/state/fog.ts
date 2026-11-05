// The current world's fog of war (see `lib/fog`): loaded with the world, revealed
// around the player on the surface as they move, saved every few seconds.

import { emit, listen } from '@tauri-apps/api/event'
import { useEffect, useMemo } from 'react'
import { create } from 'zustand'
import { logUiError } from '../lib/errors'
import { emptyFog, exploredAt, loadFog, mergeFog, reveal, saveFog, type Fog, type SavedFog } from '../lib/fog'
import { FRIENDS_CATEGORY, type Pin } from '../lib/game'
import { useSettings } from '../settings/store'
import { getStore } from './persist'
import { useStore } from './store'
import { keys } from './worlds'

/** How often new reveals are saved. */
const SAVE_MS = 5000

interface FogState {
  /** `fog:<game>:<world>` the fog belongs to. */
  key: string | null
  fog: Fog | null
  /** Bumped on every change (the bitmap is mutated in place). */
  version: number
  /**
   * A copy of the fog, refreshed at most every `SETTLE_MS` while exploring and right
   * away on loads and resets: for work too heavy for every step (filtering pins).
   */
  settled: Fog | null
}

export const useFogStore = create<FogState>(() => ({ key: null, fog: null, version: 0, settled: null }))

const settle = (fog: Fog | null): Fog | null => (fog ? { ...fog, bits: fog.bits.slice() } : null)

/** How often pins in newly explored areas appear while exploring. */
const SETTLE_MS = 2000
let settleTimer: ReturnType<typeof setTimeout> | null = null

/** The fog changed: bump `version` now and refresh `settled` soon (or now). */
function changed(now = false) {
  useFogStore.setState((s) => ({ version: s.version + 1, settled: now ? settle(s.fog) : s.settled }))
  if (now || settleTimer) return
  settleTimer = setTimeout(() => {
    settleTimer = null
    useFogStore.setState((s) => ({ settled: settle(s.fog) }))
  }, SETTLE_MS)
}

let dirty = false

/** Saves reveals not saved yet. */
export async function flushFog(): Promise<void> {
  await save()
}

async function save() {
  const { key, fog } = useFogStore.getState()
  if (!dirty || !key || !fog) return
  dirty = false
  try {
    await (await getStore()).set(key, saveFog(fog))
    void emit(SAVED, key)
  } catch (e) {
    // Try again with the next save.
    dirty = true
    logUiError(`could not save explored areas: ${String(e)}`)
  }
}

/** The overlay saved new reveals (the map window reloads them). */
const SAVED = 'fog-saved'
/** A window forgot the explored areas of a world. */
const RESET = 'fog-reset'

/** The explored areas to send to others, once there are any. */
export function currentFog(): SavedFog | null {
  const { fog } = useFogStore.getState()
  return fog && fog.bits.some((b) => b !== 0) ? saveFog(fog) : null
}

/** Adds what another player has explored in this world. */
export function mergeFriendFog(saved: SavedFog) {
  const { fog } = useFogStore.getState()
  if (!fog) return
  const other = loadFog(saved, { ...fog, bits: new Uint8Array(fog.bits.length) })
  if (mergeFog(fog, other)) {
    dirty = true
    changed(true)
  }
}

/** Forgets the current world's explored areas. */
export async function resetFog() {
  const { key, fog } = useFogStore.getState()
  if (!key || !fog) return
  fog.bits.fill(0)
  changed(true)
  await (await getStore()).set(key, saveFog(fog))
  // To every window: the one recording would otherwise save its copy right back.
  void emit(RESET, key)
}

/**
 * Whether fog of war hides a pin: a game pin in an unexplored part of the map, unless
 * it's found, tracked or selected. `null` while pins aren't hidden. Updates as the
 * player explores (every few seconds).
 */
export function usePinFog(): ((pin: Pin) => boolean) | null {
  const enabled = useSettings((s) => s.ui.fogOfWar && s.ui.fogHidePins)
  const fog = useFogStore((s) => s.settled)
  const projection = useStore((s) => s.map?.projection ?? null)
  const found = useStore((s) => s.user.found)
  const tracked = useStore((s) => (s.tracked?.kind === 'pin' ? s.tracked.id : null))
  const selected = useStore((s) => s.selectedPinId)
  return useMemo(() => {
    if (!enabled || !projection) return null
    const isFound = new Set(found)
    // Until the world's fog has loaded, nothing counts as explored (no spoiling flash).
    return (pin: Pin) =>
      !pin.custom &&
      pin.category !== FRIENDS_CATEGORY &&
      pin.id !== tracked &&
      pin.id !== selected &&
      !isFound.has(pin.id) &&
      !(fog && exploredAt(fog, pin.position[projection.east], pin.position[projection.north]))
  }, [enabled, fog, projection, found, tracked, selected])
}

/** Loads the fog of the shown world, and (when `record`) reveals it as the player moves. */
export function useFog(record: boolean) {
  const gameId = useStore((s) => s.game?.id ?? null)
  const worldId = useStore((s) => s.world.id)
  const map = useStore((s) => s.map)
  const unitsPerMeter = useStore((s) => s.game?.unitsPerMeter ?? 1)

  useEffect(() => {
    if (!gameId || !map) return
    const key = keys.fog(gameId, worldId)
    let cancelled = false
    const reload = async () => {
      const saved = await (await getStore()).get<SavedFog>(key)
      if (cancelled) return
      useFogStore.setState({ key, fog: loadFog(saved, emptyFog(map.projection, unitsPerMeter)) })
      changed(true)
    }
    void save().then(reload)
    // Windows that don't record follow the one that does.
    const unlisten = record ? null : listen<string>(SAVED, ({ payload }) => void (payload === key && reload()))
    const unlistenReset = listen<string>(RESET, ({ payload }) => {
      if (payload !== key) return
      dirty = false
      void reload()
    })
    return () => {
      cancelled = true
      void unlisten?.then((f) => f())
      void unlistenReset.then((f) => f())
    }
  }, [gameId, worldId, map, unitsPerMeter, record])

  const revealMeters = useSettings((s) => s.ui.fogRadius)
  useEffect(() => {
    if (!record || !map) return
    const radius = revealMeters * unitsPerMeter
    const p = map.projection
    // Where the fog was last revealed; starts over with each world's fog.
    let last: { fog: Fog; e: number; n: number } | null = null
    const unsubscribe = useStore.subscribe((s, prev) => {
      const local = s.snapshot?.local
      const { fog, key } = useFogStore.getState()
      // Only the surface: underground layouts are small and drawn in full anyway.
      if (s.snapshot === prev.snapshot || !local || local.layer !== 'surface' || !fog || !s.game) return
      // While a new world's fog loads, the loaded fog still belongs to the previous one.
      if (key !== keys.fog(s.game.id, s.world.id)) return
      // The game is in a world the map hasn't switched to yet: this spot isn't in this one.
      if (s.snapshot?.world && s.snapshot.world.id !== s.world.id) return
      const e = local.position[p.east]
      const n = local.position[p.north]
      if (last?.fog === fog && Math.hypot(e - last.e, n - last.n) < fog.cell / 2) return
      last = { fog, e, n }
      if (reveal(fog, e, n, radius)) {
        dirty = true
        changed()
      }
    })
    const timer = setInterval(() => void save(), SAVE_MS)
    return () => {
      unsubscribe()
      clearInterval(timer)
      void save()
    }
  }, [record, map, unitsPerMeter, revealMeters])
}
