// Persists settings and per-game user data (custom pins, found pins, filters) with
// the Tauri store plugin. Writes are debounced by the plugin's autosave.

import { load, type Store } from '@tauri-apps/plugin-store'
import type { Hotkey } from '../lib/protocol'
import { DEFAULT_SETTINGS, EMPTY_USER_DATA, useStore, type GameUserData, type Settings } from './store'

let store: Store | null = null

async function getStore(): Promise<Store> {
  store ??= await load('user-data.json', { defaults: {}, autoSave: 400 })
  return store
}

/**
 * Loads UI settings. Also returns the map hotkey older versions stored here (one for
 * all games); it now lives per game in the backend's settings.
 */
export async function loadSettings(): Promise<{ settings: Settings; legacyHotkey: Hotkey | null }> {
  const s = await getStore()
  const { hotkey, ...saved } = (await s.get<Partial<Settings> & { hotkey?: Hotkey | null }>('settings')) ?? {}
  return { settings: { ...DEFAULT_SETTINGS, ...saved, hud: { ...DEFAULT_SETTINGS.hud, ...saved.hud } }, legacyHotkey: hotkey ?? null }
}

/** Drops the old all-games hotkey once it has been moved to the backend. */
export async function clearLegacyHotkey(): Promise<void> {
  const s = await getStore()
  await s.set('settings', useStore.getState().settings)
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
