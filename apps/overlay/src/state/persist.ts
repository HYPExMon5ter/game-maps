// Persists settings and per-game user data (custom pins, found pins, filters) with
// the Tauri store plugin. Writes are debounced by the plugin's autosave.

import { load, type Store } from '@tauri-apps/plugin-store'
import { DEFAULT_SETTINGS, EMPTY_USER_DATA, useStore, type GameUserData, type Settings } from './store'

let store: Store | null = null

async function getStore(): Promise<Store> {
  store ??= await load('user-data.json', { defaults: {}, autoSave: 400 })
  return store
}

export async function loadSettings(): Promise<Settings> {
  const s = await getStore()
  const saved = (await s.get<Partial<Settings>>('settings')) ?? {}
  return { ...DEFAULT_SETTINGS, ...saved, hud: { ...DEFAULT_SETTINGS.hud, ...saved.hud } }
}

export async function loadUserData(gameId: string): Promise<GameUserData> {
  const s = await getStore()
  return { ...EMPTY_USER_DATA, ...((await s.get<Partial<GameUserData>>(`game:${gameId}`)) ?? {}) }
}

/** Saves settings and the current game's user data whenever they change. */
export function startPersisting(): () => void {
  return useStore.subscribe((state, prev) => {
    if (state.settings !== prev.settings) {
      void getStore().then((s) => s.set('settings', state.settings))
    }
    if (state.game && state.user !== prev.user && state.game === prev.game) {
      const key = `game:${state.game.id}`
      void getStore().then((s) => s.set(key, state.user))
    }
  })
}
