// Typed wrappers around Tauri commands and events exposed by `src-tauri`.

import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type { AgentLog, Hotkey, LinkStatus, OverlayMode, PinConfig, TeleportTarget, WorldSnapshot } from './protocol'

export const backend = {
  status: () => invoke<LinkStatus>('get_status'),
  lastSnapshot: () => invoke<WorldSnapshot | null>('get_last_snapshot'),
  teleport: (target: TeleportTarget) => invoke<void>('teleport', { target }),
  /** Dev tooling: scans the loaded world; resolves to the saved file path. */
  exportWorld: () => invoke<string>('export_world'),
  setHotkeys: (hotkeys: Hotkey[]) => invoke<void>('set_hotkeys', { hotkeys }),
  overlayMode: () => invoke<OverlayMode>('get_overlay_mode'),
  closeOverlay: () => invoke<void>('close_overlay'),
  setPinConfig: (config: PinConfig) => invoke<void>('set_pin_config', { config }),
  listGames: () => invoke<string[]>('list_games'),
  saveUserIcon: (fileName: string, bytes: Uint8Array) =>
    invoke<string>('save_user_icon', { fileName, bytes: Array.from(bytes) }),
}

export const events = {
  status: (cb: (s: LinkStatus) => void): Promise<UnlistenFn> => listen<LinkStatus>('agent-status', (e) => cb(e.payload)),
  snapshot: (cb: (s: WorldSnapshot) => void): Promise<UnlistenFn> =>
    listen<WorldSnapshot>('world-snapshot', (e) => cb(e.payload)),
  overlayMode: (cb: (m: OverlayMode) => void): Promise<UnlistenFn> =>
    listen<OverlayMode>('overlay-mode', (e) => cb(e.payload)),
  agentLog: (cb: (l: AgentLog) => void): Promise<UnlistenFn> => listen<AgentLog>('agent-log', (e) => cb(e.payload)),
}
