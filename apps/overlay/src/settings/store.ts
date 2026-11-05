// Every setting the user can change, for both windows (map and launcher): the UI's
// own (`Settings`, saved with the user data) and the backend's (`AppSettings`: keys,
// how each game connects, updates). `startSettings` loads and syncs them.

import { useMemo } from 'react'
import { create } from 'zustand'
import type { AppSettings, GameEntry, HookMode } from '../lib/backend'
import type { Hotkey } from '../lib/protocol'
import { DEFAULT_SETTINGS, gameHud, gameSettings, withGameSettings, type GameSettings, type HudSettings, type MinimapLayout, type Onboarding, type Settings } from './model'

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

/** The HUD settings as they apply to a game (no game: the shared ones). */
export function useGameHud(gameId: string | null | undefined): HudSettings {
  const hud = useSettings((s) => s.ui.hud)
  const layout = useSettings((s) => (gameId ? s.ui.games[gameId]?.minimapLayout : null))
  return useMemo(() => gameHud(hud, layout), [hud, layout])
}

/** Places the mini-map for a game, or for every game without its own placement. */
export function setMinimapLayout(gameId: string | null | undefined, layout: MinimapLayout) {
  const { setGameSettings, setHud } = useSettings.getState()
  if (gameId) setGameSettings(gameId, { minimapLayout: layout })
  else setHud(layout)
}

/** Marks a one-time message as dealt with. */
export function finishOnboarding(step: keyof Onboarding) {
  const { ui, setUi } = useSettings.getState()
  setUi({ onboarding: { ...ui.onboarding, [step]: true } })
}

/** How the app connects to a game: its own choice, or the one for all games. */
export function hookMode(app: AppSettings | null, gameId: string | null | undefined): HookMode {
  return (gameId && app?.hook_modes[gameId]) || app?.default_hook_mode || 'auto'
}

/** The key that opens a game's map: the user's choice or the game's default. */
export function mapHotkey(app: AppSettings | null, game: { id: string; defaultHotkeys: Hotkey[] } | null): Hotkey | null {
  if (!game) return null
  return app?.map_hotkeys[game.id] ?? game.defaultHotkeys[0] ?? null
}
