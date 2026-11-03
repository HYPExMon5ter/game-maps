// Every setting the user can change, for both windows (map and launcher): the UI's
// own (`Settings`, saved with the user data) and the backend's (`AppSettings`: keys,
// how each game connects, updates). `startSettings` loads and syncs them.

import { create } from 'zustand'
import type { AppSettings, GameEntry } from '../lib/backend'
import type { Hotkey } from '../lib/protocol'
import { DEFAULT_SETTINGS, gameSettings, withGameSettings, type GameSettings, type HudSettings, type Settings } from './model'

interface SettingsState {
  ui: Settings
  /** Backend settings; null until loaded. */
  app: AppSettings | null
  /** Starts with Windows; null until loaded. */
  autostart: boolean | null
  /** Supported games (for per-game settings). */
  games: GameEntry[]

  setUi(patch: Partial<Settings>): void
  setHud(patch: Partial<HudSettings>): void
  setGameSettings(gameId: string, patch: Partial<GameSettings>): void
  setApp(app: AppSettings): void
  setAutostart(enabled: boolean): void
}

export const useSettings = create<SettingsState>((set) => ({
  ui: DEFAULT_SETTINGS,
  app: null,
  autostart: null,
  games: [],

  setUi: (patch) => set((s) => ({ ui: { ...s.ui, ...patch } })),
  setHud: (patch) => set((s) => ({ ui: { ...s.ui, hud: { ...s.ui.hud, ...patch } } })),
  setGameSettings: (gameId, patch) => set((s) => ({ ui: withGameSettings(s.ui, gameId, patch) })),
  setApp: (app) => set({ app }),
  setAutostart: (autostart) => set({ autostart }),
}))

/** One of a game's settings (no game: the default). */
export function useGameSetting<K extends keyof GameSettings>(gameId: string | null | undefined, key: K): GameSettings[K] {
  return useSettings((s) => gameSettings(s.ui, gameId)[key])
}

/** The key that opens a game's map: the user's choice or the game's default. */
export function mapHotkey(app: AppSettings | null, game: { id: string; defaultHotkeys: Hotkey[] } | null): Hotkey | null {
  if (!game) return null
  return app?.map_hotkeys[game.id] ?? game.defaultHotkeys[0] ?? null
}
