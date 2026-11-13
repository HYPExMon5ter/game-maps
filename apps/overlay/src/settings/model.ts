// What the user can set in the map UI, and how saved settings from older versions
// are read. Two scopes: `Settings` applies to every game; `GameSettings` is kept per
// game (under `Settings.games`). Settings the backend owns (keys, how each game
// connects, updates) are `AppSettings` in `lib/backend`.

import type { Units } from '../lib/compass'
import type { FractionRect } from '../lib/layout'
import type { Corner, Hotkey } from '../lib/protocol'

export interface HudSettings {
  /** Keep a mini-map on screen while playing. */
  minimap: boolean
  /** Where it sits: 0..1 across the room on the screen (see `anchoredRect`). */
  anchor: { x: number; y: number }
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
  /** Fade the mini-map out while danger markers or pings are on or next to it. */
  autoHide: boolean
  /** Show the in-game time, day and weather on the mini-map. */
  clock: boolean
}

/** Where the mini-map sits and how big it is. */
export type MinimapLayout = Pick<HudSettings, 'anchor' | 'width' | 'height'>

/** Settings for one game; what a game shows or does differs between games. */
export interface GameSettings {
  /** This game's mini-map placement; null uses the one in `HudSettings`. */
  minimapLayout: MinimapLayout | null
  /** Collectibles are marked found when the player passes them. */
  autoFound: boolean
  /** Dots for creatures around the player (as far as the game simulates them). */
  showCreatures: boolean
  /** Include wildlife, not just enemies and companions. */
  showAnimals: boolean
  /** Connect with others running the app in the same multiplayer session. */
  playTogether: boolean
  /** Point toward hostile creatures close by, over the game. */
  dangerAlert: boolean
  /** How close hostiles must be for the danger alert, in meters. */
  dangerMeters: number
  /** Draw map pings over the game, like the waypoint. */
  pingsInGame: boolean
  /** Put the player's pins and pings on the game's own map too. */
  gameMap: boolean
  /** Valheim: show silver the player hasn't found with a wishbone. */
  showSilver: boolean
  /** Valheim: teleport with what portals don't let through. */
  teleportRestricted: boolean
  /** Valheim: a timer on picked berries and plants until they're back. */
  resourceTimers: boolean
  /** Valheim: send a hosted world's locations to friends who join. */
  shareLocations: boolean
  /** Share found items and explored areas with others in the same world. */
  shareProgress: boolean
  /** Show where other players are over the game. */
  friendMarkers: boolean
  /** Drop a pin where the player last died. */
  deathMarker: boolean
}

/** One-time messages already dealt with. */
export interface Onboarding {
  /** Chose how to play (see `lib/playStyle`). */
  welcomed: boolean
  /** Saw what the dark parts of the map are. */
  fogTip: boolean
  /** Answered the request to star the project. */
  star: boolean
}

/** Settings for every game. */
export interface Settings {
  onboarding: Onboarding
  /** Offer teleporting, in games that can. */
  teleport: boolean
  /** Opacity of the full map (the game shows through below 1). */
  opacity: number
  /** Where the floating map sits on the screen. */
  mapPanel: FractionRect
  /** The map fills the screen instead of floating. */
  mapMaximized: boolean
  /** Interface scale; null picks one from the screen size. */
  uiScale: number | null
  /** Pin badge size multiplier (1 = default; badges also scale with zoom). */
  pinSize: number
  /** Overlapping pins give way to more important ones, which leave a dot behind. */
  declutter: boolean
  units: Units
  /** Show the walking route to what you track (map and mini-map). */
  showRoute: boolean
  /** Show where you've walked this session. */
  showTrail: boolean
  /** The full map in 3D: tilted terrain on the surface, cave and bunker models underground. */
  map3d: boolean
  /** Keep track of the parts of the map you've been near (see `fogHideMap`, `fogHidePins`). */
  fogOfWar: boolean
  /** Fog of war: cover unexplored parts of the map (else they're only shaded). */
  fogHideMap: boolean
  /** Fog of war: hide game pins in unexplored parts of the map. */
  fogHidePins: boolean
  /** Fog of war: how far around the player the map is revealed, in meters. */
  fogRadius: number
  hud: HudSettings
  /** Per game, by game id; missing values use `DEFAULT_GAME_SETTINGS`. */
  games: Record<string, Partial<GameSettings>>
}

export const DEFAULT_GAME_SETTINGS: GameSettings = {
  minimapLayout: null,
  autoFound: true,
  showCreatures: true,
  showAnimals: true,
  playTogether: true,
  dangerAlert: true,
  dangerMeters: 30,
  pingsInGame: true,
  gameMap: true,
  showSilver: false,
  teleportRestricted: false,
  resourceTimers: false,
  shareLocations: true,
  shareProgress: false,
  friendMarkers: true,
  deathMarker: true,
}

/** A map panel in the middle of the screen, `scale` of its size. */
export function centeredPanel(scale: number): FractionRect {
  return { x: (1 - scale) / 2, y: (1 - scale) / 2, width: scale, height: scale }
}

/** The anchor of a screen corner. */
export function cornerAnchor(corner: Corner): { x: number; y: number } {
  return { x: corner.endsWith('right') ? 1 : 0, y: corner.startsWith('bottom') ? 1 : 0 }
}

export const DEFAULT_SETTINGS: Settings = {
  onboarding: { welcomed: false, fogTip: false, star: false },
  teleport: true,
  opacity: 0.96,
  mapPanel: centeredPanel(0.8),
  mapMaximized: false,
  uiScale: null,
  pinSize: 1,
  declutter: true,
  units: 'imperial',
  showRoute: true,
  showTrail: true,
  map3d: false,
  fogOfWar: true,
  fogHideMap: true,
  fogHidePins: true,
  fogRadius: 100,
  hud: { minimap: false, anchor: { x: 1, y: 0 }, width: 300, height: 300, zoom: 0, opacity: 0.92, rotateWithPlayer: false, waypoint: true, autoHide: true, clock: true },
  games: {},
}

/** A game's settings, defaults filled in. */
export function gameSettings(settings: Settings, gameId: string | null | undefined): GameSettings {
  return { ...DEFAULT_GAME_SETTINGS, ...(gameId ? settings.games[gameId] : undefined) }
}

/** The HUD settings for a game: its own mini-map placement over the shared ones. */
export function gameHud(hud: HudSettings, layout: MinimapLayout | null | undefined): HudSettings {
  return layout ? { ...hud, ...layout } : hud
}

export function withGameSettings(settings: Settings, gameId: string, patch: Partial<GameSettings>): Settings {
  return { ...settings, games: { ...settings.games, [gameId]: { ...settings.games[gameId], ...patch } } }
}

/** Per-game settings older versions kept for all games. */
const LEGACY_GAME_KEYS = ['autoFound', 'showCreatures', 'showAnimals', 'playTogether'] as const
const SETTING_KEYS = Object.keys(DEFAULT_SETTINGS) as (keyof Settings)[]

/** Saved settings as older versions may have left them. */
export type SavedSettings = Partial<Omit<Settings, 'hud'>> & Partial<GameSettings> & { hud?: SavedHud; hotkey?: Hotkey | null; followPlayer?: boolean; panelScale?: number }

/** Saved HUD settings: older versions had one `size`, and a `corner` to sit in. */
type SavedHud = Partial<HudSettings> & { size?: number; corner?: Corner }

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
  // Settings from before these messages: this isn't a first run.
  if (saved && !raw.onboarding) settings.onboarding = { welcomed: true, fogTip: true, star: false }
  // Older versions scaled the map around the screen's center.
  if (!raw.mapPanel && raw.panelScale !== undefined && raw.panelScale < 1) settings.mapPanel = centeredPanel(raw.panelScale)
  const legacy: Partial<GameSettings> = {}
  for (const key of LEGACY_GAME_KEYS) {
    const value = raw[key]
    if (typeof value === 'boolean') legacy[key] = value
  }
  if (Object.keys(legacy).length) {
    const games = { ...settings.games }
    for (const id of gameIds) games[id] = { ...legacy, ...games[id] }
    settings.games = games
  }
  return { settings, legacyHotkey: raw.hotkey ?? null }
}

/** Saved HUD settings with defaults filled in. */
export function migrateHud(saved: SavedHud | undefined): HudSettings {
  const { size, corner, ...rest } = saved ?? {}
  const square = size === undefined ? {} : { width: size, height: size }
  const anchor = corner ? { anchor: cornerAnchor(corner) } : {}
  return { ...DEFAULT_SETTINGS.hud, ...square, ...anchor, ...rest }
}
