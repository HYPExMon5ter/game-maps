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
  /** Pins can go on the game's own map. */
  game_map?: boolean
  /** The game has a ping of its own. */
  game_pings?: boolean
}

export interface LocalPlayer {
  position: Vec3
  forward: Vec3 | null
  layer: WorldLayer
  /** The game's main camera (absent from older agents). */
  camera?: CameraView | null
}

/** A walking route from the game's navigation mesh. */
export interface PathInfo {
  corners: Vec3[]
  /** False when the destination can't be reached; the route ends as close as it gets. */
  complete: boolean
}

/** Which legs of a route can't be walked right now (see `backend.checkRoute`). */
export interface RouteCheck {
  /** Indices `i` of blocked legs, from `points[i]` to `points[i + 1]`. */
  blocked: number[]
  /** Legs checked, from the start: long routes take several checks. */
  checked: number
}

/** The floor under a probed point (see `backend.probeGround`). */
export interface GroundPoint {
  /** Height of the floor nearest the point, if there's one close by. */
  y: number | null
  /** Whether the game camera can see that floor point. */
  visible: boolean
}

export interface CameraView {
  position: Vec3
  forward: Vec3
  up: Vec3
  /** Vertical field of view, degrees. */
  fov_y: number
  /** Width / height of the camera's image. */
  aspect: number
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
  /** Creatures the game has active around the player; absent when there are none. */
  creatures?: Creature[]
  /** The multiplayer session the player is in; absent in single player. */
  session?: MultiplayerSession
  /** The world or save that's loaded; absent in menus or when the game doesn't expose it. */
  world?: GameWorld
  /** AI companions the game reports details for; absent when there are none. */
  companions?: CompanionStatus[]
  /** Time, season and weather, for games that expose them. */
  environment?: Environment
  /** Where the player's things are after dying, while they're on the ground. */
  dropped_bag?: Vec3
  /** The player is dying or dead right now. */
  dying?: boolean
  /** Inside a dungeon, in games where that isn't its own map layer. */
  indoors?: boolean
  /** Milestones reached in this world, as the game's checklist names them. */
  progress?: string[]
  /** Places the game counts as discovered. */
  visited?: Vec3[]
}

/** What changed since the last snapshot; `markers` and `creatures` only when they did. */
export interface SnapshotUpdate {
  frame: number
  local: LocalPlayer | null
  others: RemotePlayer[]
  markers?: Marker[]
  creatures?: Creature[]
  companions?: CompanionStatus[]
  environment?: Environment
  dropped_bag?: Vec3
  dying?: boolean
  indoors?: boolean
  progress?: string[]
  visited?: Vec3[]
}

export interface CompanionStat {
  name: string
  value: number
  max: number
}

export interface CompanionStatus {
  id: string
  name: string
  position: Vec3 | null
  alive: boolean
  injured: boolean
  /** What they're doing, when the game says. */
  activity: string | null
  stats: CompanionStat[]
}

/** Something a companion can be told to do. */
export interface PlaceItem {
  name: string
  /** Only known for one-off finds. */
  taken: boolean
}

export interface PlaceLoot {
  title: string
  items: PlaceItem[]
}

/** What the game's own map says about a place. */
export interface PlaceInfo {
  name: string
  level: number
  /** Percent scavenged; null if there's nothing to scavenge. */
  scavenged: number | null
  loot: PlaceLoot[]
}

export interface CompanionOrderOption {
  id: string
  /** Heading it's listed under. */
  group: string
  label: string
}

export interface Environment {
  /** Days survived, when the game counts them. */
  day: number | null
  hour: number
  minute: number
  season: string | null
  weather: string | null
  /** For games with wind that matters. */
  wind?: { degrees: number; strength: number }
}

export type WorldKind = 'single_player' | 'hosted' | 'joined'

/** A world or save (see `state/worlds`). */
export interface GameWorld {
  /** Stable for this world, unique within the game. */
  id: string
  name: string | null
  kind: WorldKind
}

export interface MultiplayerSession {
  /** The same for everyone in the session. */
  id: string
  /** This player's name as the others see it. */
  player_name: string | null
}

export type CreatureKind = 'hostile' | 'animal' | 'friendly'

export interface Creature {
  id: string
  kind: CreatureKind
  /** The game's type name (e.g. "Fingers", "Deer"). */
  name: string
  position: Vec3
}

export type HotkeyAction = 'toggle_map' | 'zoom_in' | 'zoom_out' | 'teleport_back' | 'quick_mark' | 'nearest_unfound' | 'accept_request'

export interface Hotkey {
  action: HotkeyAction
  vk: number
  ctrl?: boolean
  shift?: boolean
  alt?: boolean
}

export type TeleportTarget =
  | { kind: 'position'; position: Vec3; snap_to_ground: boolean; layer?: WorldLayer }
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
  /** Where the HUD sits: 0..1 across the room left on the screen inside the margin. */
  anchor_x: number
  anchor_y: number
  width: number
  height: number
  margin: number
  /** Cover the game's whole screen (click-through) to draw waypoints over it. */
  fullscreen: boolean
}

export interface AgentLog {
  level: 'debug' | 'info' | 'warn' | 'error'
  message: string
}
