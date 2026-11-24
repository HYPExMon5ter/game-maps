// The current world's fog of war (see `lib/fog`): loaded with the world, revealed
// around the player on the surface as they move, saved every few seconds to the world's
// own file (see `loadFogFile`).

import { emit, listen } from '@tauri-apps/api/event'
import { useEffect, useMemo } from 'react'
import { create } from 'zustand'
import { backend } from '../lib/backend'
import { logUiError } from '../lib/errors'
import { decodeFog, emptyFog, encodeFog, exploredAt, loadFog, mergeFog, reveal, saveFog, type Fog, type SavedFog } from '../lib/fog'
import { FRIENDS_CATEGORY, type Pin } from '../lib/game'
import { useSettings } from '../settings/store'
import { deleteFogFile, FOG_SAVED, loadFogFile } from './persist'
import { useStore } from './store'
import { keys } from './worlds'

/** How often new reveals are saved. */
const SAVE_MS = 5000
/** How long after a world's fog couldn't be loaded it's tried again. */
const LOAD_RETRY_MS = 5000

interface FogState {
  /** `fog:<game>:<world>` the fog belongs to. */
  key: string | null
  gameId: string | null
  worldId: string | null
  fog: Fog | null
  /** Bumped on every change (the bitmap is mutated in place). */
  version: number
  /**
   * A copy of the fog, refreshed at most every `SETTLE_MS` while exploring and right
   * away on loads and resets: for work too heavy for every step (filtering pins).
   */
  settled: Fog | null
}

export const useFogStore = create<FogState>(() => ({ key: null, gameId: null, worldId: null, fog: null, version: 0, settled: null }))

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

/** Reveals not saved yet. */
let dirty = false
/** The save that's running, if one is: they go one at a time. */
let inFlight: Promise<void> | null = null
/** Whether the last save failed (said once, not every few seconds). */
let failing = false

/** A world's fog as it would be saved. */
interface FogFile {
  gameId: string
  worldId: string
  key: string
  file: Uint8Array<ArrayBuffer>
}

/**
 * The fog of a world the map has moved on from, whose last save failed: tried again with
 * the next save, and joined with what's on disk if the map comes back to that world first.
 */
let owed: FogFile | null = null

/** Saves reveals not saved yet. Whether everything is on disk afterwards. */
export async function flushFog(): Promise<boolean> {
  return save()
}

/** The fog being recorded for a world, as it would be saved: for when saving it has failed. */
export function recordedFog(gameId: string, worldId: string): Uint8Array<ArrayBuffer> | null {
  const { key, fog } = useFogStore.getState()
  return fog && key === keys.fog(gameId, worldId) ? encodeFog(fog) : null
}

async function write({ gameId, worldId, key, file }: FogFile): Promise<boolean> {
  try {
    await backend.saveFog(gameId, worldId, file)
    failing = false
    void emit(FOG_SAVED, key)
    return true
  } catch (e) {
    logUiError(`could not save explored areas: ${String(e)}`)
    if (!failing) useStore.getState().notify('Could not save the areas you explored. They stay here while the app is open', 'error')
    failing = true
    return false
  }
}

async function save(): Promise<boolean> {
  while (inFlight) await inFlight
  let saved = true
  inFlight = (async () => {
    const earlier = owed
    if (earlier) {
      if (await write(earlier)) owed = owed === earlier ? null : owed
      else saved = false
    }
    const { key, gameId, worldId, fog } = useFogStore.getState()
    if (!dirty || !key || !gameId || !worldId || !fog) return
    dirty = false
    // To the world the fog was loaded for, whatever the map shows by the time it's written.
    const pending = { gameId, worldId, key, file: encodeFog(fog) }
    if (await write(pending)) return
    saved = false
    // Tried again with the next save: as the fog being recorded if it still is, else as
    // what's owed to the world the map has left.
    if (useFogStore.getState().fog === fog) dirty = true
    else owed = pending
  })().finally(() => {
    inFlight = null
  })
  await inFlight
  return saved
}

/** A world's fog is about to be deleted: nothing of it may be written afterwards. */
export async function dropPendingFog(gameId: string, worldId: string): Promise<void> {
  const key = keys.fog(gameId, worldId)
  const drop = () => {
    if (useFogStore.getState().key === key) dirty = false
    if (owed?.key === key) owed = null
  }
  drop()
  while (inFlight) await inFlight
  // A save that failed meanwhile asked to be tried again.
  drop()
}

/** A world's explored areas were forgotten: every window loads them again. */
const RESET = 'fog-reset'
/** A window asks the one that records to forget a world's explored areas. */
const RESET_REQUEST = 'fog-reset-request'

interface ResetRequest {
  gameId: string
  worldId: string
}

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

/**
 * Adds what the game itself counts as explored: `bits` is laid out like the fog's, one
 * bit per cell, or it's left out.
 */
export function mergeGameExplored(bits: Uint8Array) {
  const { fog } = useFogStore.getState()
  if (!fog || fog.bits.length !== bits.length) return
  if (mergeFog(fog, { ...fog, bits })) {
    dirty = true
    changed(true)
  }
}

/**
 * Forgets the current world's explored areas. Done by the window that records them (this
 * one or the other): it's the one whose saves could otherwise bring them back.
 */
export async function resetFog() {
  const { gameId, worldId } = useFogStore.getState()
  if (gameId && worldId) await emit(RESET_REQUEST, { gameId, worldId } satisfies ResetRequest)
}

async function forget({ gameId, worldId }: ResetRequest) {
  const key = keys.fog(gameId, worldId)
  const { fog } = useFogStore.getState()
  if (useFogStore.getState().key === key && fog) {
    fog.bits.fill(0)
    changed(true)
  }
  await dropPendingFog(gameId, worldId)
  await deleteFogFile(gameId, worldId)
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
    let retry: ReturnType<typeof setTimeout> | undefined
    const reload = async () => {
      clearTimeout(retry)
      try {
        // Only the window that records moves fog an older version saved: one writer.
        const file = await loadFogFile(gameId, worldId, record)
        if (cancelled) return
        const blank = emptyFog(map.projection, unitsPerMeter)
        const fog = decodeFog(file, blank)
        // What was explored here before, and couldn't be saved when the map left.
        if (owed?.key === key) {
          if (mergeFog(fog, decodeFog(owed.file, blank))) dirty = true
          owed = null
        }
        useFogStore.setState({ key, gameId, worldId, fog })
        changed(true)
      } catch (e) {
        if (cancelled) return
        // Not another world's fog in its place, and nothing recorded onto a blank one,
        // which would be saved over what's on disk.
        logUiError(`could not load explored areas: ${String(e)}`)
        if (useFogStore.getState().key !== null) {
          useFogStore.setState({ key: null, gameId: null, worldId: null, fog: null })
          changed(true)
          useStore.getState().notify('Could not load the areas you explored. Trying again', 'error')
        }
        retry = setTimeout(() => void reload(), LOAD_RETRY_MS)
      }
    }
    void save().then(reload)
    // Windows that don't record follow the one that does.
    const unlisten = record ? null : listen<string>(FOG_SAVED, ({ payload }) => void (payload === key && reload()))
    const unlistenReset = listen<string>(RESET, ({ payload }) => {
      if (payload !== key) return
      dirty = false
      void reload()
    })
    return () => {
      cancelled = true
      clearTimeout(retry)
      void unlisten?.then((f) => f())
      void unlistenReset.then((f) => f())
    }
  }, [gameId, worldId, map, unitsPerMeter, record])

  useEffect(() => {
    if (!record) return
    const unlisten = listen<ResetRequest>(RESET_REQUEST, ({ payload }) => {
      forget(payload).catch((e: unknown) => logUiError(`could not reset explored areas: ${String(e)}`))
    })
    return () => void unlisten.then((f) => f())
  }, [record])

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
