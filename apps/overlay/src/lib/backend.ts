// Typed wrappers around Tauri commands and events exposed by `src-tauri`.

import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type { AgentLog, Hotkey, LinkStatus, OverlayMode, PinConfig, TeleportTarget, WorldSnapshot } from './protocol'

/** A supported game as bundled with the app (subset of `game.json`). */
export interface GameEntry {
  id: string
  name: string
  process: string
  steamAppId: number | null
  defaultHotkeys: Hotkey[]
  /** Has live location and teleport; otherwise a static map. */
  liveFeatures: boolean
}

export type HookMode = 'auto' | 'manual'

/** Two slots per action; `null` turns a slot off. The same for every game. */
export interface ZoomKeys {
  zoom_in: [Hotkey | null, Hotkey | null]
  zoom_out: [Hotkey | null, Hotkey | null]
}

export interface AppSettings {
  hook_modes: Record<string, HookMode>
  /** Per-game map hotkey; absent uses the game's default. */
  map_hotkeys: Record<string, Hotkey>
  zoom_keys: ZoomKeys
  auto_update: boolean
}

export interface RemotePack {
  version: string
  url: string
  sha256: string
  size: number
}

export interface PackStatus {
  game_id: string
  /** Installed pack version; `"development"` when map data comes from the repository. */
  installed: string | null
  available: RemotePack | null
  downloading: { received: number; total: number } | null
  error: string | null
}

export type UpdateState =
  | { state: 'disabled' | 'idle' | 'checking' | 'up_to_date' | 'installing' }
  | { state: 'available'; version: string; notes: string | null }
  | { state: 'downloading'; received: number; total: number | null }
  | { state: 'error'; message: string }

export interface SetupChecks {
  needs_admin: boolean
  exclusive_fullscreen: boolean
}

export const backend = {
  status: () => invoke<LinkStatus>('get_status'),
  lastSnapshot: () => invoke<WorldSnapshot | null>('get_last_snapshot'),
  teleport: (target: TeleportTarget) => invoke<void>('teleport', { target }),
  /** Dev tooling: scans the loaded world; resolves to the saved file path. */
  exportWorld: () => invoke<string>('export_world'),
  overlayMode: () => invoke<OverlayMode>('get_overlay_mode'),
  openMap: () => invoke<void>('open_map'),
  closeOverlay: () => invoke<void>('close_overlay'),
  setPinConfig: (config: PinConfig) => invoke<void>('set_pin_config', { config }),
  listGames: () => invoke<GameEntry[]>('list_games'),
  saveUserIcon: (fileName: string, bytes: Uint8Array) =>
    invoke<string>('save_user_icon', { fileName, bytes: Array.from(bytes) }),

  appSettings: () => invoke<AppSettings>('get_app_settings'),
  setHookMode: (gameId: string, mode: HookMode) => invoke<AppSettings>('set_hook_mode', { gameId, mode }),
  /** `null` resets to the game's default key. */
  setMapHotkey: (gameId: string, hotkey: Hotkey | null) => invoke<AppSettings>('set_map_hotkey', { gameId, hotkey }),
  /** `null` restores the default zoom keys. */
  setZoomKeys: (keys: ZoomKeys | null) => invoke<AppSettings>('set_zoom_keys', { keys }),
  setAutoUpdate: (enabled: boolean) => invoke<AppSettings>('set_auto_update', { enabled }),
  autostart: () => invoke<boolean>('get_autostart'),
  setAutostart: (enabled: boolean) => invoke<boolean>('set_autostart', { enabled }),
  connectGame: () => invoke<void>('connect_game'),
  disconnectGame: () => invoke<void>('disconnect_game'),
  launchGame: (gameId: string) => invoke<void>('launch_game', { gameId }),
  setupChecks: () => invoke<SetupChecks>('get_setup_checks'),
  mapPacks: () => invoke<PackStatus[]>('get_map_packs'),
  installMapPack: (gameId: string) => invoke<void>('install_map_pack', { gameId }),
  updateState: () => invoke<UpdateState>('get_update_state'),
  checkForUpdates: () => invoke<void>('check_for_updates'),
  installUpdate: () => invoke<void>('install_update'),
  openLogsFolder: () => invoke<void>('open_logs_folder'),
}

export const events = {
  status: (cb: (s: LinkStatus) => void): Promise<UnlistenFn> => listen<LinkStatus>('agent-status', (e) => cb(e.payload)),
  snapshot: (cb: (s: WorldSnapshot) => void): Promise<UnlistenFn> =>
    listen<WorldSnapshot>('world-snapshot', (e) => cb(e.payload)),
  overlayMode: (cb: (m: OverlayMode) => void): Promise<UnlistenFn> =>
    listen<OverlayMode>('overlay-mode', (e) => cb(e.payload)),
  agentLog: (cb: (l: AgentLog) => void): Promise<UnlistenFn> => listen<AgentLog>('agent-log', (e) => cb(e.payload)),
  mapPacks: (cb: (p: PackStatus[]) => void): Promise<UnlistenFn> => listen<PackStatus[]>('map-packs', (e) => cb(e.payload)),
  mapPackInstalled: (cb: (gameId: string) => void): Promise<UnlistenFn> =>
    listen<string>('map-pack-installed', (e) => cb(e.payload)),
  appSettings: (cb: (s: AppSettings) => void): Promise<UnlistenFn> => listen<AppSettings>('app-settings', (e) => cb(e.payload)),
  updateState: (cb: (s: UpdateState) => void): Promise<UnlistenFn> => listen<UpdateState>('update-state', (e) => cb(e.payload)),
  /** A zoom key was pressed in the game while the mini-map is shown; `steps` is +1 or -1. */
  mapZoom: (cb: (steps: number) => void): Promise<UnlistenFn> => listen<number>('map-zoom', (e) => cb(e.payload)),
}
