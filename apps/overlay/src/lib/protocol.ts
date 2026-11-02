// Mirrors `crates/protocol` and the Tauri command/event payloads in `src-tauri`.
// Field names follow the Rust serde output (snake_case).

export interface Vec3 {
  x: number
  y: number
  z: number
}

export type WorldLayer = 'surface' | 'caves' | 'other'

export interface Capabilities {
  live_location: boolean
  teleport: boolean
  other_players: boolean
  hotkeys: boolean
}

export interface LocalPlayer {
  position: Vec3
  forward: Vec3 | null
  layer: WorldLayer
}

export interface RemotePlayer {
  id: string
  name: string
  color: [number, number, number] | null
  position: Vec3
  forward: Vec3 | null
}

export type MarkerKind = 'companion' | 'locator'

/** Live marker from the game's own map/GPS (companions, placed locators, vehicles). */
export interface Marker {
  id: string
  kind: MarkerKind
  label: string | null
  /** Game icon name (`games/<id>/icons/game/<icon>.png`). */
  icon: string | null
  position: Vec3
}

export interface WorldSnapshot {
  frame: number
  local: LocalPlayer | null
  others: RemotePlayer[]
  /** Absent from older agents. */
  markers?: Marker[]
}

export type HotkeyAction = 'toggle_map'

export interface Hotkey {
  action: HotkeyAction
  vk: number
  ctrl?: boolean
  shift?: boolean
  alt?: boolean
}

export type TeleportTarget =
  | { kind: 'position'; position: Vec3; snap_to_ground: boolean }
  | { kind: 'player'; id: string }

/** `ready`: the game is running but not connected (manual hooking, or disconnected). */
export type LinkState = 'waiting_for_game' | 'ready' | 'connecting' | 'connected' | 'error'

export interface LinkStatus {
  state: LinkState
  game_id: string | null
  pid: number | null
  capabilities: Capabilities | null
  message: string | null
}

export type OverlayMode = 'hidden' | 'open' | 'pinned'

export type Corner = 'top_left' | 'top_right' | 'bottom_left' | 'bottom_right'

export interface PinConfig {
  enabled: boolean
  corner: Corner
  width: number
  height: number
  margin: number
}

export interface AgentLog {
  level: 'debug' | 'info' | 'warn' | 'error'
  message: string
}
