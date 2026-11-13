// Typed wrappers around Tauri commands and events exposed by `src-tauri`.

import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type { Pin } from './game'
import type { AgentLog, CompanionOrderOption, GroundPoint, Hotkey, HotkeyAction, LinkStatus, OverlayMode, PathInfo, PinConfig, PlaceInfo, RouteCheck, SnapshotUpdate, TeleportTarget, Vec3, WorldSnapshot } from './protocol'

/** A supported game as bundled with the app (subset of `game.json`). */
export interface GameEntry {
  id: string
  name: string
  process: string
  steamAppId: number | null
  defaultHotkeys: Hotkey[]
  /** Has live location and teleport; otherwise a static map. */
  liveFeatures: boolean
  /** Pins and pings can go on the game's own map. */
  gameMap?: boolean
}

export type HookMode = 'auto' | 'manual'

/** Two slots per action; `null` turns a slot off. The same for every game. */
export interface ZoomKeys {
  zoom_in: [Hotkey | null, Hotkey | null]
  zoom_out: [Hotkey | null, Hotkey | null]
}

export interface ActionKeys {
  quick_mark: Hotkey | null
  nearest_unfound: Hotkey | null
  accept_request: Hotkey | null
}

export interface AppSettings {
  /** For games without a choice of their own in `hook_modes`. */
  default_hook_mode: HookMode
  hook_modes: Record<string, HookMode>
  /** Per-game map hotkey; absent uses the game's default. */
  map_hotkeys: Record<string, Hotkey>
  zoom_keys: ZoomKeys
  /** Returns to where you were before your last teleport (all games); null = off. */
  back_key: Hotkey | null
  /** Keys for what the map does itself (all games); null = off. */
  action_keys: ActionKeys
  auto_update: boolean
  /** Monitor the map window is kept open on; null when it's off. */
  map_window: string | null
  /** Whether anonymous usage counts may be sent; null until the player has answered. */
  analytics: boolean | null
}

export interface MonitorInfo {
  name: string
  width: number
  height: number
  primary: boolean
}

export interface RemotePack {
  version: string
  url: string
  sha256: string
  size: number
  /** Optional parts of the map published for download on request, by name (`3d`). */
  extras?: Record<string, RemotePack>
}

/** An optional part of a map (see `RemotePack.extras`) on this computer. */
export interface ExtraStatus {
  /** Installed version (`"development"` from the repository); null until asked for. */
  installed: string | null
  downloading: { received: number; total: number } | null
  error: string | null
}

export interface PackStatus {
  game_id: string
  /** Installed pack version; `"development"` when map data comes from the repository. */
  installed: string | null
  available: RemotePack | null
  downloading: { received: number; total: number } | null
  error: string | null
  extras: Record<string, ExtraStatus>
}

export type UpdateState =
  | { state: 'disabled' | 'idle' | 'checking' | 'up_to_date' | 'installing' }
  | { state: 'available'; version: string; notes: string | null }
  | { state: 'downloading'; received: number; total: number | null }
  | { state: 'error'; message: string }

export interface SetupChecks {
  needs_admin: boolean
  exclusive_fullscreen: boolean
  map_outdated: boolean
}

export const backend = {
  status: () => invoke<LinkStatus>('get_status'),
  lastSnapshot: () => invoke<WorldSnapshot | null>('get_last_snapshot'),
  teleport: (target: TeleportTarget) => invoke<void>('teleport', { target }),
  /** Back to where the player was before their last teleport (one step per call). */
  teleportBack: () => invoke<void>('teleport_back'),
  /** Brings the game's own map in line with the player's pins. */
  setGamePins: (pins: GamePin[]) => invoke<void>('set_game_pins', { pins }),
  /** The player's options for this game that the agent acts on. */
  setGameOptions: (options: Record<string, boolean>) => invoke<void>('set_game_options', { options }),
  /** Keeps the locations this world's host shared. */
  saveHostPins: (pins: Pin[]) => invoke<void>('save_host_pins', { pins }),
  /** Pings a spot the way the game does, for everyone in the world. */
  gamePing: (position: Vec3) => invoke<void>('game_ping', { position }),
  /** A walking route from the player to `to`, on the game's navigation mesh. */
  findPath: (to: Vec3) => invoke<PathInfo>('find_path', { to }),
  /** Which legs of a route (floor points about a meter apart) can't be walked right now. */
  checkRoute: (points: Vec3[]) => invoke<RouteCheck>('check_route', { points }),
  /** The floor under points near the player, and whether the game camera sees them. */
  probeGround: (points: Vec3[]) => invoke<GroundPoint[]>('probe_ground', { points }),
  /** What the game says about the place at `position`. */
  placeInfo: (position: Vec3) => invoke<PlaceInfo>('place_info', { position }),
  /** What a companion can be told to do right now. */
  companionOrders: (companion: string) => invoke<CompanionOrderOption[]>('companion_orders', { companion }),
  companionOrder: (companion: string, order: string) => invoke<void>('companion_order', { companion, order }),
  /** How many teleports `teleportBack` can undo. */
  teleportBackSteps: () => invoke<number>('teleport_back_steps'),
  /** Dev tooling: scans the loaded world; resolves to the saved file path. */
  exportWorld: () => invoke<string>('export_world'),
  overlayMode: () => invoke<OverlayMode>('get_overlay_mode'),
  openMap: (gameId: string) => invoke<void>('open_map', { gameId }),
  closeOverlay: () => invoke<void>('close_overlay'),
  setPinConfig: (config: PinConfig) => invoke<void>('set_pin_config', { config }),
  listGames: () => invoke<GameEntry[]>('list_games'),
  saveUserIcon: (fileName: string, bytes: Uint8Array) =>
    invoke<string>('save_user_icon', { fileName, bytes: Array.from(bytes) }),

  appSettings: () => invoke<AppSettings>('get_app_settings'),
  setHookMode: (gameId: string, mode: HookMode) => invoke<AppSettings>('set_hook_mode', { gameId, mode }),
  setAllHookModes: (mode: HookMode) => invoke<AppSettings>('set_all_hook_modes', { mode }),
  openProjectPage: () => invoke<void>('open_project_page'),
  openPrivacyPage: () => invoke<void>('open_privacy_page'),
  openPosthogPage: () => invoke<void>('open_posthog_page'),
  /** `null` resets to the game's default key. */
  setMapHotkey: (gameId: string, hotkey: Hotkey | null) => invoke<AppSettings>('set_map_hotkey', { gameId, hotkey }),
  /** `null` restores the default zoom keys. */
  setZoomKeys: (keys: ZoomKeys | null) => invoke<AppSettings>('set_zoom_keys', { keys }),
  /** `null` turns the teleport back key off. */
  setBackKey: (key: Hotkey | null) => invoke<AppSettings>('set_back_key', { key }),
  /** `null` turns the action's key off. */
  setActionKey: (action: HotkeyAction, key: Hotkey | null) => invoke<AppSettings>('set_action_key', { action, key }),
  /** Asks where to save a text file; false when the user cancels. */
  saveTextFile: (name: string, contents: string) => invoke<boolean>('save_text_file', { name, contents }),
  saveReport: (ui: string) => invoke<boolean>('save_report', { ui }),
  openIssuePage: () => invoke<void>('open_issue_page'),
  setAutoUpdate: (enabled: boolean) => invoke<AppSettings>('set_auto_update', { enabled }),
  setAnalytics: (enabled: boolean) => invoke<AppSettings>('set_analytics', { enabled }),
  /** Notes a feature used during play, for the session's usage counts. */
  countFeature: (feature: 'tracked' | 'multiplayer') => invoke<void>('count_feature', { feature }),
  /** Writes a UI error to the app's log. */
  logUiError: (message: string) => invoke<void>('log_ui_error', { message }),
  /** Writes a UI event to the app's log. */
  logUiInfo: (message: string) => invoke<void>('log_ui_info', { message }),
  listMonitors: () => invoke<MonitorInfo[]>('list_monitors'),
  /** Keeps the map window open on a monitor, or closes it (`null`). */
  setMapWindow: (monitor: string | null) => invoke<AppSettings>('set_map_window', { monitor }),
  autostart: () => invoke<boolean>('get_autostart'),
  setAutostart: (enabled: boolean) => invoke<boolean>('set_autostart', { enabled }),
  connectGame: () => invoke<void>('connect_game'),
  disconnectGame: () => invoke<void>('disconnect_game'),
  launchGame: (gameId: string) => invoke<void>('launch_game', { gameId }),
  setupChecks: () => invoke<SetupChecks>('get_setup_checks'),
  mapPacks: () => invoke<PackStatus[]>('get_map_packs'),
  installMapPack: (gameId: string) => invoke<void>('install_map_pack', { gameId }),
  /** Downloads an optional part of a game's map (`3d`). */
  installMapExtra: (gameId: string, extra: string) => invoke<void>('install_map_extra', { gameId, extra }),
  updateState: () => invoke<UpdateState>('get_update_state'),
  checkForUpdates: () => invoke<void>('check_for_updates'),
  installUpdate: () => invoke<void>('install_update'),
  openLogsFolder: () => invoke<void>('open_logs_folder'),
}

/** A pin for the game's own map. */
export interface GamePin {
  name: string
  position: Vec3
}

export interface WorldMapReady {
  game_id: string
  world_id: string
  /** The tiles are new, not only the pins. */
  tiles: boolean
  /** There's a new `explored.bin`. */
  explored: boolean
}

export const events = {
  status: (cb: (s: LinkStatus) => void): Promise<UnlistenFn> => listen<LinkStatus>('agent-status', (e) => cb(e.payload)),
  snapshot: (cb: (s: WorldSnapshot) => void): Promise<UnlistenFn> =>
    listen<WorldSnapshot>('world-snapshot', (e) => cb(e.payload)),
  snapshotUpdate: (cb: (u: SnapshotUpdate) => void): Promise<UnlistenFn> =>
    listen<SnapshotUpdate>('world-update', (e) => cb(e.payload)),
  overlayMode: (cb: (m: OverlayMode) => void): Promise<UnlistenFn> =>
    listen<OverlayMode>('overlay-mode', (e) => cb(e.payload)),
  agentLog: (cb: (l: AgentLog) => void): Promise<UnlistenFn> => listen<AgentLog>('agent-log', (e) => cb(e.payload)),
  mapPacks: (cb: (p: PackStatus[]) => void): Promise<UnlistenFn> => listen<PackStatus[]>('map-packs', (e) => cb(e.payload)),
  mapPackInstalled: (cb: (gameId: string) => void): Promise<UnlistenFn> =>
    listen<string>('map-pack-installed', (e) => cb(e.payload)),
  mapExtraInstalled: (cb: (e: { game_id: string; extra: string }) => void): Promise<UnlistenFn> =>
    listen<{ game_id: string; extra: string }>('map-extra-installed', (e) => cb(e.payload)),
  appSettings: (cb: (s: AppSettings) => void): Promise<UnlistenFn> => listen<AppSettings>('app-settings', (e) => cb(e.payload)),
  updateState: (cb: (s: UpdateState) => void): Promise<UnlistenFn> => listen<UpdateState>('update-state', (e) => cb(e.payload)),
  /** A world's map files changed (see `state/worldMaps`). */
  worldMapReady: (cb: (r: WorldMapReady) => void): Promise<UnlistenFn> => listen<WorldMapReady>('world-map-ready', (e) => cb(e.payload)),
  /** The launcher asked for this game's map. */
  showGame: (cb: (gameId: string) => void): Promise<UnlistenFn> => listen<string>('show-game', (e) => cb(e.payload)),
  /** The teleport back key was used in the game; the payload is the error, if any. */
  teleportBack: (cb: (error: string | null) => void): Promise<UnlistenFn> => listen<string | null>('teleport-back', (e) => cb(e.payload)),
  /** A key for something the map does itself was pressed in the game. */
  hotkeyAction: (cb: (action: HotkeyAction) => void): Promise<UnlistenFn> => listen<HotkeyAction>('hotkey-action', (e) => cb(e.payload)),
  /** A zoom key was pressed in the game while the mini-map is shown; `steps` is +1 or -1. */
  mapZoom: (cb: (steps: number) => void): Promise<UnlistenFn> => listen<number>('map-zoom', (e) => cb(e.payload)),
}
