// Worlds: a game's saves or servers, each with its own found items, custom pins, fog of
// war and map state. The agent reports which world is loaded (`WorldSnapshot.world`);
// games that don't, and anything recorded before a world was first detected, use the
// game's default world.
//
// Storage (user-data.json):
//   prefs:<game>            category filters (the same in every world)
//   worlds:<game>           known worlds, the last one played, and whether the default
//                           world's data has moved to a detected world yet
//   game:<game>             the default world's pins and found items (where every
//                           game's data lived before worlds)
//   world:<game>:<world>    a detected world's pins and found items
//   session:<game>[:<world>] map state, `fog:<game>:<world>` explored areas

import type { Pin } from '../lib/game'
import type { GameWorld, WorldKind } from '../lib/protocol'

/** The world of games that don't report one. */
export const DEFAULT_WORLD = 'default'

/**
 * A world loaded without a save of its own yet (a new game before it's first saved).
 * Not listed; its data moves to the save once there is one, or goes with it.
 */
export function isUnsaved(worldId: string): boolean {
  return worldId.startsWith('unsaved:')
}

export interface WorldInfo {
  id: string
  name: string | null
  kind: WorldKind | null
  /** When it was last loaded in the game (ms since the epoch); 0 for the default world. */
  lastPlayed: number
}

export interface WorldIndex {
  worlds: WorldInfo[]
  /** The world shown when no game is running. */
  last: string | null
  /** Whether the default world's data has moved to the first detected world. */
  claimed: boolean
}

export const EMPTY_INDEX: WorldIndex = { worlds: [], last: null, claimed: false }

export const DEFAULT_WORLD_INFO: WorldInfo = { id: DEFAULT_WORLD, name: null, kind: null, lastPlayed: 0 }

/** A world's own data (see `GamePrefs` for what every world shares). */
export interface WorldData {
  customPins: Pin[]
  found: string[]
}

export interface GamePrefs {
  hiddenCategories: string[]
  shownCategories: string[]
}

export const EMPTY_WORLD_DATA: WorldData = { customPins: [], found: [] }
export const EMPTY_PREFS: GamePrefs = { hiddenCategories: [], shownCategories: [] }

export const keys = {
  prefs: (game: string) => `prefs:${game}`,
  index: (game: string) => `worlds:${game}`,
  data: (game: string, world: string) => (world === DEFAULT_WORLD ? `game:${game}` : `world:${game}:${world}`),
  session: (game: string, world: string) => (world === DEFAULT_WORLD ? `session:${game}` : `session:${game}:${world}`),
  fog: (game: string, world: string) => `fog:${game}:${world}`,
}

const KIND_LABELS: Record<WorldKind, string> = { single_player: 'Single player', hosted: 'Hosting', joined: 'Joined' }

/** How a world is shown: its name, or what kind of save it is. */
export function worldLabel(w: WorldInfo): string {
  if (w.id === DEFAULT_WORLD) return 'Default world'
  if (isUnsaved(w.id)) return 'New game (not saved yet)'
  if (w.name) return w.name
  const number = w.id.split(':').pop()
  return `${w.kind ? KIND_LABELS[w.kind] : 'World'} save ${number}`
}

export function worldKindLabel(kind: WorldKind | null): string | null {
  return kind ? KIND_LABELS[kind] : null
}

/** The index after `world` was loaded in the game at `now`. */
export function recordPlayed(index: WorldIndex, world: GameWorld, now: number): WorldIndex {
  const info: WorldInfo = { id: world.id, name: world.name, kind: world.kind, lastPlayed: now }
  const rest = index.worlds.filter((w) => w.id !== world.id)
  return { ...index, worlds: [info, ...rest], last: world.id }
}

/** Whether data is worth keeping (and moving). */
export function hasData(d: WorldData): boolean {
  return d.customPins.length > 0 || d.found.length > 0
}

/** `a` plus what `b` has that `a` doesn't. */
export function mergeWorldData(a: WorldData, b: WorldData): WorldData {
  const pinIds = new Set(a.customPins.map((p) => p.id))
  return { customPins: [...a.customPins, ...b.customPins.filter((p) => !pinIds.has(p.id))], found: [...new Set([...a.found, ...b.found])] }
}

/** Worlds to offer in the world picker: most recently played first, the default world last if it has data. */
export function pickerWorlds(index: WorldIndex, defaultHasData: boolean): WorldInfo[] {
  const played = [...index.worlds].sort((a, b) => b.lastPlayed - a.lastPlayed)
  return defaultHasData || played.length === 0 ? [...played, DEFAULT_WORLD_INFO] : played
}
