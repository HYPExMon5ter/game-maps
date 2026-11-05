// Persists per-game filters and per-world data (custom pins, found pins, map session; see
// `state/worlds` for the keys) with the Tauri store plugin. Writes are debounced by the
// plugin's autosave. Settings are saved in the same store (see `settings/sync`).

import { emit, listen } from '@tauri-apps/api/event'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { load, type Store } from '@tauri-apps/plugin-store'
import { logUiError } from '../lib/errors'
import type { GameWorld } from '../lib/protocol'
import { EMPTY_SESSION, selectSession, useStore, type GameUserData, type MapSession, type SavedMapView, type TrackTarget } from './store'
import {
  DEFAULT_WORLD,
  EMPTY_INDEX,
  EMPTY_PREFS,
  EMPTY_WORLD_DATA,
  hasData,
  isUnsaved,
  keys,
  mergeWorldData,
  recordPlayed,
  type GamePrefs,
  type WorldData,
  type WorldIndex,
} from './worlds'

let store: Store | null = null

export async function getStore(): Promise<Store> {
  store ??= await load('user-data.json', { defaults: {}, autoSave: 400 })
  return store
}

/** Category filters. Older versions kept them with the pins under `game:<id>`. */
export async function loadPrefs(gameId: string): Promise<GamePrefs> {
  const s = await getStore()
  const saved = await s.get<Partial<GamePrefs>>(keys.prefs(gameId))
  if (saved) return { ...EMPTY_PREFS, ...saved }
  const legacy = (await s.get<Partial<GamePrefs>>(keys.data(gameId, DEFAULT_WORLD))) ?? {}
  const prefs = { ...EMPTY_PREFS, hiddenCategories: legacy.hiddenCategories ?? [], shownCategories: legacy.shownCategories ?? [] }
  await s.set(keys.prefs(gameId), prefs)
  return prefs
}

export async function loadWorldData(gameId: string, worldId: string): Promise<WorldData> {
  const s = await getStore()
  const saved = (await s.get<Partial<WorldData>>(keys.data(gameId, worldId))) ?? {}
  return { ...EMPTY_WORLD_DATA, ...saved }
}

export async function loadSession(gameId: string, worldId: string): Promise<MapSession> {
  const s = await getStore()
  const saved = await s.get<Partial<MapSession>>(keys.session(gameId, worldId))
  const legacy = worldId === DEFAULT_WORLD ? legacyMapView(gameId) : null
  // Live ids only mean something within one run of the game.
  return { ...EMPTY_SESSION, mapView: legacy, ...(saved ?? {}), selectedLiveId: null }
}

export async function loadWorldIndex(gameId: string): Promise<WorldIndex> {
  const s = await getStore()
  return { ...EMPTY_INDEX, ...((await s.get<Partial<WorldIndex>>(keys.index(gameId))) ?? {}) }
}

/** The map view older versions kept in the web view's local storage. */
function legacyMapView(gameId: string): SavedMapView | null {
  try {
    const v = JSON.parse(localStorage.getItem(`game-maps:view:${gameId}`) ?? 'null') as SavedMapView | null
    return v && Array.isArray(v.center) && v.center.length === 2 && Number.isFinite(v.zoom) ? v : null
  } catch {
    return null
  }
}

/**
 * Records that `world` was loaded in the game. The first world detected takes over what
 * the default world has (everything from before worlds were told apart) unless it
 * already has data of its own.
 */
export async function enterWorld(gameId: string, world: GameWorld, index: WorldIndex): Promise<WorldIndex> {
  if (isUnsaved(world.id)) return index
  const s = await getStore()
  let next = recordPlayed(index, world, Date.now())
  if (!next.claimed) {
    const [fallback, own] = await Promise.all([loadWorldData(gameId, DEFAULT_WORLD), loadWorldData(gameId, world.id)])
    if (hasData(fallback)) {
      // Prefs move out of `game:<id>` first (see `loadPrefs`), so clearing it loses nothing.
      await loadPrefs(gameId)
      // The world may have data already (a new game played before its first save):
      // pins and found items join it; map state and explored areas only fill gaps.
      await s.set(keys.data(gameId, world.id), mergeWorldData(own, fallback))
      for (const key of ['session', 'fog'] as const) {
        const value = await s.get(keys[key](gameId, DEFAULT_WORLD))
        if (value !== undefined && (await s.get(keys[key](gameId, world.id))) === undefined) await s.set(keys[key](gameId, world.id), value)
      }
      await s.set(keys.data(gameId, DEFAULT_WORLD), EMPTY_WORLD_DATA)
      await s.delete(keys.session(gameId, DEFAULT_WORLD))
      await s.delete(keys.fog(gameId, DEFAULT_WORLD))
    }
    next = { ...next, claimed: true }
  }
  await s.set(keys.index(gameId), next)
  return next
}

const WORLD_KEYS = ['data', 'session', 'fog'] as const

/**
 * A new game was saved for the first time: what was recorded before (`from`, an
 * unsaved world) becomes the save's, unless it already has data of its own.
 */
export async function adoptUnsaved(gameId: string, from: string, to: string): Promise<void> {
  const s = await getStore()
  if (!hasData(await loadWorldData(gameId, to))) {
    for (const key of WORLD_KEYS) {
      const value = await s.get(keys[key](gameId, from))
      if (value !== undefined) await s.set(keys[key](gameId, to), value)
    }
  }
  await discardWorld(gameId, from)
}

/** Deletes everything kept for a world. */
export async function discardWorld(gameId: string, worldId: string): Promise<void> {
  const s = await getStore()
  for (const key of WORLD_KEYS) await s.delete(keys[key](gameId, worldId))
}

/** Deletes what's left of new games that were never saved. */
export async function discardUnsavedWorlds(gameId: string): Promise<void> {
  const s = await getStore()
  const prefixes = [keys.data(gameId, 'unsaved:'), keys.session(gameId, 'unsaved:'), keys.fog(gameId, 'unsaved:')]
  for (const key of await s.keys()) {
    if (prefixes.some((p) => key.startsWith(p))) await s.delete(key)
  }
}

/** Shows `worldId` when no game is running. */
export async function rememberLastWorld(gameId: string, index: WorldIndex, worldId: string): Promise<WorldIndex> {
  const next = { ...index, last: worldId }
  await (await getStore()).set(keys.index(gameId), next)
  return next
}

/** The user's data for the map: every world's filters plus this world's pins. */
export function userData(prefs: GamePrefs, data: WorldData): GameUserData {
  // Prefs last: old `game:<id>` data can still hold filters that moved to them.
  return { ...data, ...prefs }
}

/** A write that failed is written again with the next change: say so, don't lose it quietly. */
function saving(write: Promise<unknown>) {
  write.catch((e: unknown) => {
    logUiError(`could not save: ${String(e)}`)
    useStore.getState().notify('Could not save your map data. It stays here while the app is open', 'error')
  })
}

const SESSION_KEYS = Object.keys(EMPTY_SESSION) as (keyof MapSession)[]

/** Window to window (overlay and map window): pins, found items or tracking changed. */
const CHANGED = 'map-data'

interface Changed {
  from: string
  gameId: string
  worldId: string
  user?: GameUserData
  tracked?: TrackTarget | null
}

/**
 * Saves the current game's filters and the current world's data and map session
 * whenever they change, and keeps the other window's map in step (pins, found items
 * and what's tracked; each window keeps its own view).
 */
export function startPersisting(): () => void {
  const self = getCurrentWebviewWindow().label
  // Changes from the other window are saved there already.
  let applyingRemote = false
  const unlisten = listen<Changed>(CHANGED, ({ payload: c }) => {
    const s = useStore.getState()
    if (c.from === self || s.game?.id !== c.gameId || s.world.id !== c.worldId) return
    applyingRemote = true
    if (c.user) useStore.setState({ user: c.user })
    if (c.tracked !== undefined) s.setTracked(c.tracked)
    applyingRemote = false
  })
  const unsubscribe = useStore.subscribe((state, prev) => {
    // Loading another game or world replaces everything at once; nothing to save.
    if (applyingRemote || !state.game || state.game !== prev.game || state.world !== prev.world) return
    const gameId = state.game.id
    const worldId = state.world.id
    const changed: Changed = { from: self, gameId, worldId }
    if (state.user !== prev.user) {
      const { hiddenCategories, shownCategories, filterSets, customPins, found, notes, timers } = state.user
      saving(
        getStore().then((s) =>
          Promise.all([s.set(keys.prefs(gameId), { hiddenCategories, shownCategories, filterSets }), s.set(keys.data(gameId, worldId), { customPins, found, notes, timers })]),
        ),
      )
      changed.user = state.user
    }
    if (state.tracked !== prev.tracked) changed.tracked = state.tracked
    if (SESSION_KEYS.some((k) => state[k] !== prev[k])) {
      const session = selectSession(state)
      saving(getStore().then((s) => s.set(keys.session(gameId, worldId), session)))
    }
    if (changed.user || changed.tracked !== undefined) void emit(CHANGED, changed)
  })
  return () => {
    unsubscribe()
    void unlisten.then((f) => f())
  }
}
