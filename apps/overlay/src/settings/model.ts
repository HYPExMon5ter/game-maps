// What the user can set in the map UI, and how saved settings from older versions
// are read. Two scopes: `Settings` applies to every game; `GameSettings` is kept per
// game (under `Settings.games`). Settings the backend owns (keys, how each game
// connects, updates) are `AppSettings` in `lib/backend`.

import type { Units } from '../lib/compass'
import type { Corner, Hotkey } from '../lib/protocol'

export interface HudSettings {
  /** Keep a mini-map on screen while playing. */
  minimap: boolean
  corner: Corner
  /** Mini-map size in logical pixels (capped at half the screen, see `hudSize`). */
  width: number
  height: number
  /** Mini-map zoom in levels relative to the default (about 1 screen pixel per world unit). */
  zoom: number
  opacity: number
  /** Rotate the mini-map so the player's view direction points up. */
  rotateWithPlayer: boolean
  /** Draw the tracked target over the game where it is in the world. */
  waypoint: boolean
}

/** Settings for one game; what a game shows or does differs between games. */
export interface GameSettings {
  /** Collectibles are marked found when the player passes them. */
  autoFound: boolean
  /** Dots for creatures around the player (as far as the game simulates them). */
  showCreatures: boolean
  /** Include wildlife, not just enemies and companions. */
  showAnimals: boolean
  /** Connect with others running the app in the same multiplayer session. */
  playTogether: boolean
}

/** Settings for every game. */
export interface Settings {
  /** Opacity of the full map (the game shows through below 1). */
  opacity: number
  /** Fraction of the screen the full map panel uses. */
  panelScale: number
  /** Interface scale; null picks one from the screen size. */
  uiScale: number | null
  /** Pin badge size multiplier (1 = default; badges also scale with zoom). */
  pinSize: number
  /** Overlapping pins give way to more important ones, which leave a dot behind. */
  declutter: boolean
  units: Units
  /** Show the walking route to what you track (map and mini-map). */
  showRoute: boolean
  hud: HudSettings
  /** Per game, by game id; missing values use `DEFAULT_GAME_SETTINGS`. */
  games: Record<string, Partial<GameSettings>>
}

export const DEFAULT_GAME_SETTINGS: GameSettings = {
  autoFound: true,
  showCreatures: true,
  showAnimals: true,
  playTogether: true,
}

export const DEFAULT_SETTINGS: Settings = {
  opacity: 0.96,
  panelScale: 1,
  uiScale: null,
  pinSize: 1,
  declutter: true,
  units: 'imperial',
  showRoute: true,
  hud: { minimap: false, corner: 'top_right', width: 300, height: 300, zoom: 0, opacity: 0.92, rotateWithPlayer: false, waypoint: true },
  games: {},
}

/** A game's settings, defaults filled in. */
export function gameSettings(settings: Settings, gameId: string | null | undefined): GameSettings {
  return { ...DEFAULT_GAME_SETTINGS, ...(gameId ? settings.games[gameId] : undefined) }
}

export function withGameSettings(settings: Settings, gameId: string, patch: Partial<GameSettings>): Settings {
  return { ...settings, games: { ...settings.games, [gameId]: { ...settings.games[gameId], ...patch } } }
}

const GAME_KEYS = Object.keys(DEFAULT_GAME_SETTINGS) as (keyof GameSettings)[]
const SETTING_KEYS = Object.keys(DEFAULT_SETTINGS) as (keyof Settings)[]

/** Saved settings as older versions may have left them. */
export type SavedSettings = Partial<Settings> & Partial<GameSettings> & { hotkey?: Hotkey | null; followPlayer?: boolean }

/**
 * Reads saved settings. Older versions kept the per-game settings for all games at the
 * top level: those become each known game's own (unless it already has its own). Also
 * returns the map hotkey older versions stored here, which now lives per game in the
 * backend's settings.
 */
export function readSettings(saved: SavedSettings | null | undefined, gameIds: string[]): { settings: Settings; legacyHotkey: Hotkey | null } {
  const raw = saved ?? {}
  const settings: Settings = { ...DEFAULT_SETTINGS }
  for (const key of SETTING_KEYS) {
    if (raw[key] !== undefined) (settings as unknown as Record<string, unknown>)[key] = raw[key]
  }
  settings.hud = migrateHud(raw.hud)
  const legacy: Partial<GameSettings> = {}
  for (const key of GAME_KEYS) {
    if (typeof raw[key] === 'boolean') legacy[key] = raw[key]
  }
  if (Object.keys(legacy).length) {
    const games = { ...settings.games }
    for (const id of gameIds) games[id] = { ...legacy, ...games[id] }
    settings.games = games
  }
  return { settings, legacyHotkey: raw.hotkey ?? null }
}

/** Saved HUD settings with defaults filled in. Older versions had a square mini-map with one `size`. */
export function migrateHud(saved: (Partial<HudSettings> & { size?: number }) | undefined): HudSettings {
  const { size, ...rest } = saved ?? {}
  const square = size === undefined ? {} : { width: size, height: size }
  return { ...DEFAULT_SETTINGS.hud, ...square, ...rest }
}
