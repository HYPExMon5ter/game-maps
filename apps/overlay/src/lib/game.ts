// Game definitions (`games/<id>/game.json`) and pin data, served by the app's
// `gamemaps://` protocol.

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

export interface Category {
  id: string
  name: string
  color: string
  icon: string
  visible: boolean
}

export interface GameDefinition {
  id: string
  name: string
  process: string
  unitsPerMeter: number
  defaultHotkeys: Hotkey[]
  maps: GameMap[]
  categories: Category[]
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
  /** Height is unknown (placed on the map); teleports resolve the ground height. */
  snapToGround?: boolean
}

export async function loadGame(id: string): Promise<{ game: GameDefinition; pins: Pin[] }> {
  const base = `${ASSET_BASE}/games/${encodeURIComponent(id)}`
  const [gameRes, pinsRes] = await Promise.all([fetch(`${base}/game.json`), fetch(`${base}/pins.json`)])
  if (!gameRes.ok) throw new Error(`game definition for "${id}" not found`)
  const game = (await gameRes.json()) as GameDefinition
  const pins = pinsRes.ok ? (((await pinsRes.json()) as { pins: Pin[] }).pins ?? []) : []
  return { game, pins }
}

export function tileUrl(game: GameDefinition, map: GameMap, tiles: TileSource = map.tiles): string {
  return `${ASSET_BASE}/games/${encodeURIComponent(game.id)}/${tiles.url}`
}

/**
 * Resolves an icon reference:
 * - `game:<Name>`  → icon extracted from the game (`games/<id>/icons/game/<Name>.png`)
 * - `user:<path>`  → user-uploaded file (`user/<path>`)
 * - `builtin:<n>`  → drawn by the app (returns null)
 */
export function iconUrl(gameId: string, ref: string): string | null {
  const [kind, rest] = splitRef(ref)
  if (kind === 'game') return `${ASSET_BASE}/games/${encodeURIComponent(gameId)}/icons/game/${encodeURIComponent(rest)}.png`
  if (kind === 'user') return `${ASSET_BASE}/user/${rest.split('/').map(encodeURIComponent).join('/')}`
  return null
}

function splitRef(ref: string): [string, string] {
  const i = ref.indexOf(':')
  return i < 0 ? ['builtin', ref] : [ref.slice(0, i), ref.slice(i + 1)]
}
