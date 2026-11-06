// Game definitions (`games/<id>/game.json`) and pin data, served by the app's
// `gamemaps://` protocol.

import type { ChecklistEntry } from './checklist'
import { glyphUrl } from './glyphs'
import type { Projection } from './projection'
import type { Hotkey, Vec3, WorldLayer } from './protocol'

/** Base URL of the app's asset protocol (Windows WebView2 form). */
export const ASSET_BASE = 'http://gamemaps.localhost'

export interface TileSource {
  url: string
  tileSize: number
  minNativeZoom: number
  maxNativeZoom: number
  maxZoom: number
}

export interface GameMap {
  id: string
  name: string
  layers: WorldLayer[]
  background: string
  tiles: TileSource
  /** Extra imagery drawn over the base map while viewing a layer (e.g. cave layouts). */
  layerTiles?: Partial<Record<WorldLayer, TileSource>>
  projection: Projection
}

/** Display names for world layers ("caves" also covers bunkers and other underground areas). */
export const LAYER_LABELS: Record<WorldLayer, string> = { surface: 'Surface', caves: 'Underground', other: 'Other' }

/** A badge's outline: what kind of thing a pin is, before reading its symbol. */
export type BadgeShape = 'circle' | 'square' | 'diamond' | 'hexagon' | 'shield'

export interface Category {
  id: string
  name: string
  color: string
  icon: string
  /** Circle when left out. */
  shape?: BadgeShape
  visible: boolean
  /** Collectibles: marked found automatically when the player passes them. */
  autoFound?: boolean
  /** Entrances to underground areas: routes to underground targets lead through them. */
  entrance?: boolean
  /** Dungeons: marked found once the player has been inside. */
  visitInside?: boolean
  /** Filled in as the player goes, not known for the whole world. */
  remembered?: boolean
  /** Places the game has details on: scavenged share and what's there (see `PlaceDetails`). */
  placeInfo?: boolean
  /** Portals: pins with the same name link up. */
  linksByName?: boolean
  /** Listed and filtered by kind of thing, going by pin names (see `lib/filters`). */
  split?: boolean
  /** Pin names that count as another kind in a split category. */
  mergeNames?: Record<string, string>
}

export interface GameDefinition {
  id: string
  name: string
  process: string
  unitsPerMeter: number
  defaultHotkeys: Hotkey[]
  maps: GameMap[]
  categories: Category[]
  /** How the sidebar groups the categories. */
  groups?: { id: string; name: string; categories: string[] }[]
  /** What the player's things are called where they fall when they die. */
  bagName?: string
  /** Companions can be sent to a spot on the map. */
  sendCompanions?: boolean
  /** Heading of `checklist`. */
  checklistName?: string
  /** The biomes in a generated world's `biomes.bin`. */
  biomes?: { id: number; name: string }[]
  /** Worlds are generated: each world's map and pins come from the running game. */
  worldMaps?: boolean
  /** Pins and pings can go on the game's own map (see `Capabilities.game_map`). */
  gameMap?: boolean
  /** Key items worth a list of their own. */
  checklist?: ChecklistEntry[]
  /** Pin icons shown as another: game icons that don't read on a one-color badge. */
  iconAliases?: Record<string, string>
}

export interface Pin {
  id: string
  category: string
  name: string
  description?: string
  position: Vec3
  layer?: WorldLayer
  /** Icon reference overriding the category icon (see `iconUrl`). */
  icon?: string
  color?: string
  custom?: boolean
  /** A custom pin its owner shares with others in the multiplayer session. */
  shared?: boolean
  /** Height is unknown (placed on the map); teleports resolve the ground height. */
  snapToGround?: boolean
  /** Only shown if the player chooses to, until they've found it the game's way. */
  hidden?: boolean
  /** When it can be gathered again (ms since the epoch). */
  respawnAt?: number
}

export async function loadGame(id: string): Promise<{ game: GameDefinition; pins: Pin[] }> {
  const base = `${ASSET_BASE}/games/${encodeURIComponent(id)}`
  const [gameRes, pinsRes] = await Promise.all([fetch(`${base}/game.json`), fetch(`${base}/pins.json`)])
  if (!gameRes.ok) throw new Error(`game definition for "${id}" not found`)
  const game = withFriendsCategory((await gameRes.json()) as GameDefinition)
  const pins = pinsRes.ok ? (((await pinsRes.json()) as { pins: Pin[] }).pins ?? []) : []
  return { game, pins: withIconAliases(pins, game.iconAliases) }
}

/** Where a generated world's map files are served from. */
export function worldMapBase(gameId: string, worldId: string): string {
  return `${ASSET_BASE}/worlds/${encodeURIComponent(gameId)}/${encodeURIComponent(worldId)}`
}

async function pinFile(url: string): Promise<Pin[] | null> {
  const res = await fetch(url)
  return res.ok ? (((await res.json()) as { pins: Pin[] }).pins ?? []) : null
}

/** Everything the player has seen in a world, out of the game's list of locations. */
async function rememberedPins(base: string): Promise<Pin[]> {
  const res = await fetch(`${base}/resources.json`)
  if (!res.ok) return []
  return Object.values(((await res.json()) as { zones: Record<string, Pin[]> }).zones ?? {}).flat()
}

/**
 * A generated world's pins, or null if its map hasn't been made yet: the game's locations,
 * what its host shared, and what's been seen. `showHidden`: also what the game keeps hidden.
 */
export async function loadWorldPins(gameId: string, worldId: string, showHidden: boolean): Promise<Pin[] | null> {
  const base = worldMapBase(gameId, worldId)
  const [own, host, seen] = await Promise.all([pinFile(`${base}/pins.json`), pinFile(`${base}/host-pins.json`), rememberedPins(base)])
  if (!own) return null
  return mergeWorldPins(own, host ?? [], seen, showHidden)
}

/** The world's pins from each source, each once. */
export function mergeWorldPins(own: Pin[], host: Pin[], seen: Pin[], showHidden: boolean): Pin[] {
  const ids = new Set(own.map((p) => p.id))
  return [...own, ...host.filter((p) => !ids.has(p.id)), ...seen.filter((p) => showHidden || !p.hidden)]
}

/** `pins` with icons swapped as `aliases` says (pin data comes with the map pack). */
export function withIconAliases(pins: Pin[], aliases: Record<string, string> = {}): Pin[] {
  return pins.map((pin) => (pin.icon && aliases[pin.icon] ? { ...pin, icon: aliases[pin.icon] } : pin))
}

/** Category of the pins friends share in multiplayer (added to every game). */
export const FRIENDS_CATEGORY = 'friends'

/**
 * Category of what the game itself puts on its map at runtime (flags, GPS markers,
 * vehicles): no pins, but listed so it can be hidden like any other.
 */
export const GAME_MARKERS_CATEGORY = 'game-markers'

/** Categories every game gets: friends' shared pins and the game's own markers. */
function withFriendsCategory(game: GameDefinition): GameDefinition {
  if (game.categories.some((c) => c.id === FRIENDS_CATEGORY)) return game
  const added: Category[] = [
    { id: GAME_MARKERS_CATEGORY, name: 'In-game markers', color: '#475569', icon: 'builtin:pin', visible: true },
    { id: FRIENDS_CATEGORY, name: "Friends' pins", color: '#f06292', icon: 'builtin:friends', visible: true },
  ]
  return { ...game, categories: [...game.categories, ...added] }
}

/** `worldId`: the world shown, for games whose maps are per world. */
export function tileUrl(game: GameDefinition, map: GameMap, tiles: TileSource = map.tiles, worldId?: string): string {
  if (game.worldMaps && worldId) return `${worldMapBase(game.id, worldId)}/${tiles.url}`
  return `${ASSET_BASE}/games/${encodeURIComponent(game.id)}/${tiles.url}`
}

/**
 * Resolves an icon reference:
 * - `game:<Name>`  → icon extracted from the game (`games/<id>/icons/game/<Name>.png`)
 * - `user:<path>`  → user-uploaded file (`user/<path>`)
 * - `builtin:<n>`  → a symbol drawn by the app (see `lib/glyphs`)
 */
export function iconUrl(gameId: string, ref: string): string | null {
  const [kind, rest] = splitRef(ref)
  if (kind === 'game') return `${ASSET_BASE}/games/${encodeURIComponent(gameId)}/icons/game/${encodeURIComponent(rest)}.png`
  if (kind === 'user') return `${ASSET_BASE}/user/${rest.split('/').map(encodeURIComponent).join('/')}`
  return glyphUrl(rest)
}

function splitRef(ref: string): [string, string] {
  const i = ref.indexOf(':')
  return i < 0 ? ['builtin', ref] : [ref.slice(0, i), ref.slice(i + 1)]
}
