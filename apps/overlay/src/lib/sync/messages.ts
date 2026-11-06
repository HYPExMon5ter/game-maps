// What copies of the app in the same multiplayer session say to each other (see
// `lib/sync/room.ts`). Everything that arrives is validated: peers run other versions,
// and anyone who learns the session could join the room.

import type { SavedFog } from '../fog'
import { FRIENDS_CATEGORY, type Pin } from '../game'
import type { Vec3, WorldLayer } from '../protocol'

/** Bumped when messages change incompatibly; peers on another version are ignored. */
export const SYNC_VERSION = 1

/** Something to track, as sent to others. */
export interface SharedTarget {
  name: string
  position: Vec3
  layer: WorldLayer
  /** The map pin, when it's one every copy of the game's map has. */
  pinId?: string
}

/** A custom pin its owner shares with the group. */
export interface SharedPin {
  id: string
  name: string
  description?: string
  position: Vec3
  layer: WorldLayer
  category: string
  icon?: string
  color?: string
  snapToGround?: boolean
}

export type SyncMessage =
  | { type: 'hello'; version: number; name: string | null }
  /** "Track this with me." */
  | { type: 'track'; target: SharedTarget }
  /** A spot to look at, shown briefly. */
  | { type: 'ping'; position: Vec3; layer: WorldLayer }
  /** The sender's shared pins, all of them (replaces what was sent before). */
  | { type: 'pins'; pins: SharedPin[] }
  /** Map pins the sender has found (added to what's known). */
  | { type: 'found'; ids: string[] }
  /** What the sender has explored (merged with what's known). */
  | { type: 'fog'; fog: SavedFog }
  /** The host's locations, which a joining player's game doesn't have. */
  | { type: 'locations'; pins: Location[] }

/** A location as sent: id, category, name, then the position. */
export type Location = [string, string, string, number, number, number]

/** A world has about 10,000 locations. */
export const MAX_LOCATIONS = 20000

const MAX_TEXT = 2000
export const MAX_PINS = 500
const MAX_FOUND = 5000
/** Base64 of the largest fog bitmap (see `lib/fog`). */
const MAX_FOG_TEXT = 700_000
const LAYERS: readonly WorldLayer[] = ['surface', 'caves']

/** The message in `data`, or null when it isn't a valid one. */
export function parseMessage(data: unknown): SyncMessage | null {
  if (!isObject(data)) return null
  switch (data.type) {
    case 'hello':
      return typeof data.version === 'number' && (data.name === null || isText(data.name))
        ? { type: 'hello', version: data.version, name: data.name as string | null }
        : null
    case 'track': {
      const target = parseTarget(data.target)
      return target ? { type: 'track', target } : null
    }
    case 'ping':
      return isVec3(data.position) && isLayer(data.layer) ? { type: 'ping', position: data.position, layer: data.layer } : null
    case 'pins': {
      if (!Array.isArray(data.pins)) return null
      // One bad pin isn't a reason to lose the rest.
      return { type: 'pins', pins: data.pins.slice(0, MAX_PINS).map(parseSharedPin).filter((p) => p !== null) }
    }
    case 'found':
      return Array.isArray(data.ids) && data.ids.length <= MAX_FOUND && data.ids.every(isText) ? { type: 'found', ids: data.ids as string[] } : null
    case 'fog':
      return isFog(data.fog) ? { type: 'fog', fog: data.fog } : null
    case 'locations':
      return Array.isArray(data.pins) ? { type: 'locations', pins: data.pins.slice(0, MAX_LOCATIONS).filter(isLocation) } : null
    default:
      return null
  }
}

function parseTarget(t: unknown): SharedTarget | null {
  if (!isObject(t) || !isText(t.name) || !isVec3(t.position) || !isLayer(t.layer)) return null
  if (t.pinId !== undefined && !isText(t.pinId)) return null
  return { name: t.name, position: t.position, layer: t.layer, ...(t.pinId !== undefined && { pinId: t.pinId as string }) }
}

function isLocation(l: unknown): l is Location {
  return Array.isArray(l) && l.length === 6 && isText(l[0]) && isText(l[1]) && isText(l[2]) && l.slice(3).every((n) => typeof n === 'number' && Number.isFinite(n))
}

function isFog(f: unknown): f is SavedFog {
  return isObject(f) && typeof f.bits === 'string' && f.bits.length <= MAX_FOG_TEXT && [f.cell, f.minEast, f.minNorth, f.width, f.height].every((n) => typeof n === 'number' && Number.isFinite(n))
}

/** A pin as sent to us, or null when it isn't a valid one. */
export function parseSharedPin(p: unknown): SharedPin | null {
  if (!isObject(p) || !isText(p.id) || !isText(p.name) || !isVec3(p.position) || !isLayer(p.layer) || !isText(p.category)) return null
  const optionalText = (v: unknown) => v === undefined || isText(v)
  if (!optionalText(p.description) || !optionalText(p.icon) || !optionalText(p.color)) return null
  return {
    id: p.id,
    name: p.name,
    position: p.position,
    layer: p.layer,
    category: p.category,
    ...(p.description !== undefined && { description: p.description as string }),
    ...(p.icon !== undefined && { icon: p.icon as string }),
    ...(p.color !== undefined && { color: p.color as string }),
    ...(p.snapToGround === true && { snapToGround: true }),
  }
}

/** A custom pin as shared with others (icons uploaded on this computer don't travel). */
export function toSharedPin(pin: Pin): SharedPin {
  return {
    id: pin.id,
    name: pin.name.slice(0, MAX_TEXT),
    position: pin.position,
    layer: pin.layer ?? 'surface',
    category: pin.category,
    ...(pin.description && { description: pin.description.slice(0, MAX_TEXT) }),
    ...(pin.icon && !pin.icon.startsWith('user:') && { icon: pin.icon }),
    ...(pin.color && { color: pin.color }),
    ...(pin.snapToGround && { snapToGround: true }),
  }
}

/** A friend's shared pin as a pin on this map. */
export function friendPin(peerId: string, owner: string | null, pin: SharedPin): Pin {
  return {
    id: `friend:${peerId}:${pin.id}`,
    category: FRIENDS_CATEGORY,
    name: pin.name,
    description: [owner && `Shared by ${owner}`, pin.description].filter(Boolean).join('\n') || undefined,
    position: pin.position,
    layer: pin.layer,
    ...(pin.icon && { icon: pin.icon }),
    ...(pin.color && { color: pin.color }),
    ...(pin.snapToGround && { snapToGround: true }),
  }
}

function isObject(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v)
}

function isText(v: unknown): v is string {
  return typeof v === 'string' && v.length <= MAX_TEXT
}

function isVec3(v: unknown): v is Vec3 {
  return isObject(v) && [v.x, v.y, v.z].every((n) => typeof n === 'number' && Number.isFinite(n))
}

function isLayer(v: unknown): v is WorldLayer {
  return LAYERS.includes(v as WorldLayer)
}
