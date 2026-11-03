// Persists per-game user data (custom pins, found pins, filters) and the per-game map
// session (tracking, selection, view) with the Tauri store plugin. Writes are debounced
// by the plugin's autosave. Settings are saved in the same store (see `settings/sync`).

import { load, type Store } from '@tauri-apps/plugin-store'
import {
  EMPTY_SESSION,
  EMPTY_USER_DATA,
  selectSession,
  useStore,
  type GameUserData,
  type MapSession,
  type SavedMapView,
} from './store'

let store: Store | null = null

export async function getStore(): Promise<Store> {
  store ??= await load('user-data.json', { defaults: {}, autoSave: 400 })
  return store
}

export async function loadUserData(gameId: string): Promise<GameUserData> {
  const s = await getStore()
  return { ...EMPTY_USER_DATA, ...((await s.get<Partial<GameUserData>>(`game:${gameId}`)) ?? {}) }
}

export async function loadSession(gameId: string): Promise<MapSession> {
  const s = await getStore()
  const saved = await s.get<Partial<MapSession>>(`session:${gameId}`)
  return { ...EMPTY_SESSION, mapView: legacyMapView(gameId), ...(saved ?? {}) }
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

const SESSION_KEYS = Object.keys(EMPTY_SESSION) as (keyof MapSession)[]

/** Saves the current game's user data and map session whenever they change. */
export function startPersisting(): () => void {
  return useStore.subscribe((state, prev) => {
    if (!state.game || state.game !== prev.game) return
    const gameId = state.game.id
    if (state.user !== prev.user) {
      void getStore().then((s) => s.set(`game:${gameId}`, state.user))
    }
    if (SESSION_KEYS.some((k) => state[k] !== prev[k])) {
      const session = selectSession(state)
      void getStore().then((s) => s.set(`session:${gameId}`, session))
    }
  })
}
