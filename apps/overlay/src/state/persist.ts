// Persists per-game filters and per-world data (custom pins, found pins, map session; see
// `state/worlds` for the keys) with the Tauri store plugin. Settings are saved in the same
// store (see `settings/sync`). Changes go through `setKey` and `deleteKey`, which write the
// file a moment later: the plugin's own autosave doesn't say when a write fails.
//
// Explored areas (fog of war) are a file per world, kept by the backend (`src-tauri/src/fog.rs`).
// Older versions had them in the store, under `keys.fog`; they move when first read.

import { emit, listen } from '@tauri-apps/api/event'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { load, type Store } from '@tauri-apps/plugin-store'
import { backend } from '../lib/backend'
import { logUiError } from '../lib/errors'
import { encodeFog, savedToFog, type SavedFog } from '../lib/fog'
import type { GameWorld } from '../lib/protocol'
import { createSaver } from './saver'
import { EMPTY_SESSION, selectSession, useStore, type GameUserData, type MapSession, type SavedMapView, type TrackTarget } from './store'
import {
  DEFAULT_WORLD,
  EMPTY_INDEX,
  EMPTY_PREFS,
  EMPTY_WORLD_DATA,
  changedParts,
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

/** The store, for reading. Change it with `setKey` and `deleteKey`. */
export async function getStore(): Promise<Store> {
  if (!store) {
    store = await load('user-data.json', { defaults: {}, autoSave: false })
    // A window that was closed or reloaded a moment after a change never wrote it: the
    // store still has it, so it's written now.
    saver.touch()
  }
  return store
}

function reportSaveFailure(e: unknown) {
  logUiError(`could not save: ${String(e)}`)
  useStore.getState().notify('Could not save your map data. It stays here while the app is open', 'error')
}

const saver = createSaver(async () => (await getStore()).save(), { delayMs: 400, retryMs: 5000, onFailure: reportSaveFailure })

// The file is written once the store has the change: a save before that would miss it.
export async function setKey(key: string, value: unknown): Promise<void> {
  await (await getStore()).set(key, value)
  saver.touch()
}

export async function deleteKey(key: string): Promise<void> {
  await (await getStore()).delete(key)
  saver.touch()
}

/** Category filters. Older versions kept them with the pins under `game:<id>`. */
export async function loadPrefs(gameId: string): Promise<GamePrefs> {
  const s = await getStore()
  const saved = await s.get<Partial<GamePrefs>>(keys.prefs(gameId))
  if (saved) return { ...EMPTY_PREFS, ...saved }
  const legacy = (await s.get<Partial<GamePrefs>>(keys.data(gameId, DEFAULT_WORLD))) ?? {}
  const prefs = { ...EMPTY_PREFS, hiddenCategories: legacy.hiddenCategories ?? [], shownCategories: legacy.shownCategories ?? [] }
  await setKey(keys.prefs(gameId), prefs)
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
 * A world's explored areas as saved (see `encodeFog`): its file, else what an older version
 * kept in the store, which `migrate` moves to a file. `null` when it has none.
 */
export async function loadFogFile(gameId: string, worldId: string, migrate: boolean): Promise<Uint8Array<ArrayBuffer> | null> {
  const file = await backend.loadFog(gameId, worldId)
  if (file.length > 0) return file
  const key = keys.fog(gameId, worldId)
  const legacy = await (await getStore()).get<SavedFog>(key)
  if (!legacy) return null
  const moved = encodeFog(savedToFog(legacy))
  if (migrate) {
    // The old copy goes only once the new one is on disk.
    await backend.saveFog(gameId, worldId, moved)
    await deleteKey(key)
    void emit(FOG_SAVED, key)
  }
  return moved
}

/** To every window: a world's explored areas changed on disk (the map window loads them again). */
export const FOG_SAVED = 'fog-saved'

/** Forgets a world's explored areas (see `dropPendingFog` for the fog being recorded). */
export async function deleteFogFile(gameId: string, worldId: string): Promise<void> {
  await backend.deleteFog(gameId, worldId)
  const key = keys.fog(gameId, worldId)
  if (await (await getStore()).has(key)) await deleteKey(key)
}

/**
 * `from`'s explored areas become `to`'s; with `keep`, only if `to` has none of its own.
 * `latest`: `from`'s as they are now, when saving them last failed and its file is behind.
 */
async function moveFog(gameId: string, from: string, to: string, keep: boolean, latest?: Uint8Array<ArrayBuffer> | null): Promise<void> {
  const file = latest ?? (await loadFogFile(gameId, from, false))
  if (file && !(keep && (await loadFogFile(gameId, to, false)))) {
    await backend.saveFog(gameId, to, file)
    void emit(FOG_SAVED, keys.fog(gameId, to))
  }
  await deleteFogFile(gameId, from)
}

/**
 * Records that `world` was loaded in the game. The first world detected takes over what
 * the default world has (everything from before worlds were told apart): pins and found
 * items join its own, map state and explored areas only fill gaps.
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
      // The world may have data already (a new game played before its first save).
      await setKey(keys.data(gameId, world.id), mergeWorldData(own, fallback))
      await setKey(keys.data(gameId, DEFAULT_WORLD), EMPTY_WORLD_DATA)
    }
    // Each on its own: a player may have explored or moved the map without placing a pin.
    const legacyView = legacyMapView(gameId)
    const session = (await s.get(keys.session(gameId, DEFAULT_WORLD))) ?? (legacyView ? { ...EMPTY_SESSION, mapView: legacyView } : undefined)
    if (session !== undefined) {
      if ((await s.get(keys.session(gameId, world.id))) === undefined) await setKey(keys.session(gameId, world.id), session)
      await deleteKey(keys.session(gameId, DEFAULT_WORLD))
    }
    await moveFog(gameId, DEFAULT_WORLD, world.id, true)
    next = { ...next, claimed: true }
  }
  await setKey(keys.index(gameId), next)
  return next
}

const WORLD_KEYS = ['data', 'session'] as const

/**
 * A new game was saved for the first time: what was recorded before (`from`, an
 * unsaved world) becomes the save's, unless it already has data of its own. `fog`: what
 * was explored, when it couldn't be saved (see `moveFog`).
 */
export async function adoptUnsaved(gameId: string, from: string, to: string, fog?: Uint8Array<ArrayBuffer> | null): Promise<void> {
  const s = await getStore()
  if (!hasData(await loadWorldData(gameId, to))) {
    for (const key of WORLD_KEYS) {
      const value = await s.get(keys[key](gameId, from))
      if (value !== undefined) await setKey(keys[key](gameId, to), value)
    }
    await moveFog(gameId, from, to, false, fog)
  }
  await discardWorld(gameId, from)
}

/** Deletes everything kept for a world. */
export async function discardWorld(gameId: string, worldId: string): Promise<void> {
  for (const key of WORLD_KEYS) await deleteKey(keys[key](gameId, worldId))
  await deleteFogFile(gameId, worldId)
}

/** Deletes what's left of new games that were never saved. */
export async function discardUnsavedWorlds(gameId: string): Promise<void> {
  const s = await getStore()
  const prefixes = [keys.data(gameId, 'unsaved:'), keys.session(gameId, 'unsaved:'), keys.fog(gameId, 'unsaved:')]
  for (const key of await s.keys()) {
    if (prefixes.some((p) => key.startsWith(p))) await deleteKey(key)
  }
  await backend.deleteUnsavedFog(gameId)
}

/** Shows `worldId` when no game is running. */
export async function rememberLastWorld(gameId: string, index: WorldIndex, worldId: string): Promise<WorldIndex> {
  const next = { ...index, last: worldId }
  await setKey(keys.index(gameId), next)
  return next
}

/** The user's data for the map: every world's filters plus this world's pins. */
export function userData(prefs: GamePrefs, data: WorldData): GameUserData {
  // Prefs last: old `game:<id>` data can still hold filters that moved to them.
  return { ...data, ...prefs }
}

/** Keys of the user data whose last write failed: written again with the user's next change. */
const owed = new Set<string>()
/** The latest write of each key: an older one finishing late doesn't speak for it. */
const latestWrite = new Map<string, number>()

function writeUserPart(key: string, value: unknown): Promise<void> {
  const n = (latestWrite.get(key) ?? 0) + 1
  latestWrite.set(key, n)
  const settle = (failed: boolean) => {
    if (latestWrite.get(key) !== n) return
    if (failed) owed.add(key)
    else owed.delete(key)
  }
  return setKey(key, value).then(
    () => settle(false),
    (e: unknown) => {
      settle(true)
      throw e
    },
  )
}

/** A write that failed is written again with the next change: say so, don't lose it quietly. */
function saving(write: Promise<unknown>) {
  write.catch(reportSaveFailure)
}

const SESSION_KEYS = Object.keys(EMPTY_SESSION) as (keyof MapSession)[]

/** Window to window (overlay and map window): pins, found items or tracking changed. */
const CHANGED = 'map-data'

interface Changed {
  from: string
  gameId: string
  worldId: string
  /**
   * Only what changed: both windows change the user data on their own (pins reached are
   * marked found in each), so sending all of it would undo the other window's last change.
   */
  user?: Partial<GameUserData>
  tracked?: TrackTarget | null
}

function changedFields(next: GameUserData, prev: GameUserData): Partial<GameUserData> {
  const keys = Object.keys(next) as (keyof GameUserData)[]
  return Object.fromEntries(keys.filter((k) => next[k] !== prev[k]).map((k) => [k, next[k]]))
}

/**
 * Saves the current game's filters and the current world's data and map session
 * whenever they change, and keeps the other window's map in step (pins, found items
 * and what's tracked; each window keeps its own view).
 */
export function startPersisting(): () => void {
  const self = getCurrentWebviewWindow().label
  // Changes from the other window aren't sent back. They're saved again here, merged with
  // this window's, so a save from the other window's older copy can't be the last word.
  let applyingRemote = false
  const unlisten = listen<Changed>(CHANGED, ({ payload: c }) => {
    const s = useStore.getState()
    if (c.from === self || s.game?.id !== c.gameId || s.world.id !== c.worldId) return
    applyingRemote = true
    if (c.user) useStore.setState({ user: { ...s.user, ...c.user } })
    if (c.tracked !== undefined) s.setTracked(c.tracked)
    applyingRemote = false
  })
  const unsubscribe = useStore.subscribe((state, prev) => {
    // Loading another game or world replaces everything at once; nothing to save.
    if (!state.game || state.game !== prev.game || state.world !== prev.world) return
    const gameId = state.game.id
    const worldId = state.world.id
    const changed: Changed = { from: self, gameId, worldId }
    if (state.user !== prev.user) {
      const [prefsKey, dataKey] = [keys.prefs(gameId), keys.data(gameId, worldId)]
      const { prefs, data } = changedParts(state.user, prev.user, { prefs: owed.has(prefsKey), data: owed.has(dataKey) })
      saving(Promise.all([prefs && writeUserPart(prefsKey, prefs), data && writeUserPart(dataKey, data)]))
      changed.user = changedFields(state.user, prev.user)
    }
    if (state.tracked !== prev.tracked) changed.tracked = state.tracked
    if (SESSION_KEYS.some((k) => state[k] !== prev[k])) {
      const session = selectSession(state)
      saving(setKey(keys.session(gameId, worldId), session))
    }
    if (!applyingRemote && (changed.user || changed.tracked !== undefined)) void emit(CHANGED, changed)
  })
  return () => {
    unsubscribe()
    void unlisten.then((f) => f())
  }
}
