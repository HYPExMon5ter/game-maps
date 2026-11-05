// Pin sets as files, to share outside a multiplayer session or keep as a backup.

import type { Pin } from './game'
import { parseSharedPin, toSharedPin, type SharedPin } from './sync/messages'

const FORMAT = 'game-maps-pins'
const VERSION = 1
const MAX_IMPORT = 2000

export interface PinFile {
  format: typeof FORMAT
  version: number
  game: string
  pins: SharedPin[]
}

export function exportPins(gameId: string, pins: Pin[]): string {
  const file: PinFile = { format: FORMAT, version: VERSION, game: gameId, pins: pins.map(toSharedPin) }
  return JSON.stringify(file, null, 2)
}

/** The pins in a file's text; throws with a message for the user when it can't be used. */
export function importPins(text: string, gameId: string): SharedPin[] {
  let data: unknown
  try {
    data = JSON.parse(text)
  } catch {
    throw new Error("That isn't a pins file")
  }
  const file = data as Partial<PinFile> | null
  if (file?.format !== FORMAT || !Array.isArray(file.pins)) throw new Error("That isn't a pins file")
  if (file.game !== gameId) throw new Error(`Those pins are for another game (${file.game})`)
  if (file.pins.length > MAX_IMPORT) throw new Error('That file has too many pins')
  return file.pins.map(parseSharedPin).filter((p) => p !== null)
}

/**
 * Imported pins as custom pins with ids of their own, leaving out any already there
 * (same name, same place), so importing a file twice changes nothing.
 */
export function newPinsFrom(imported: SharedPin[], existing: Pin[], makeId: () => string): Pin[] {
  const key = (p: Pick<Pin, 'name' | 'position'>) => `${p.name}@${p.position.x.toFixed(1)},${p.position.z.toFixed(1)}`
  const seen = new Set(existing.map(key))
  const out: Pin[] = []
  for (const pin of imported) {
    if (seen.has(key(pin))) continue
    seen.add(key(pin))
    out.push({ ...pin, id: makeId(), custom: true })
  }
  return out
}
