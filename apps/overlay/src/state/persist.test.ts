import { beforeEach, describe, expect, it, vi } from 'vitest'

// The store plugin and the backend's fog files, in memory.
const saved = vi.hoisted(() => ({ store: new Map<string, unknown>(), fog: new Map<string, Uint8Array>(), failFogSaves: false }))

vi.mock('@tauri-apps/plugin-store', () => ({
  load: () =>
    Promise.resolve({
      get: (key: string) => Promise.resolve(saved.store.get(key)),
      has: (key: string) => Promise.resolve(saved.store.has(key)),
      keys: () => Promise.resolve([...saved.store.keys()]),
      set: (key: string, value: unknown) => Promise.resolve(void saved.store.set(key, value)),
      delete: (key: string) => Promise.resolve(saved.store.delete(key)),
      save: () => Promise.resolve(),
    }),
}))
vi.mock('@tauri-apps/api/event', () => ({ emit: () => Promise.resolve(), listen: () => Promise.resolve(() => {}) }))
vi.mock('@tauri-apps/api/webviewWindow', () => ({ getCurrentWebviewWindow: () => ({ label: 'overlay' }) }))
vi.mock('../lib/backend', () => ({
  backend: {
    loadFog: (game: string, world: string) => Promise.resolve(saved.fog.get(`${game}/${world}`) ?? new Uint8Array()),
    saveFog: (game: string, world: string, file: Uint8Array) => (saved.failFogSaves ? Promise.reject(new Error('disk full')) : Promise.resolve(void saved.fog.set(`${game}/${world}`, file))),
    deleteFog: (game: string, world: string) => Promise.resolve(void saved.fog.delete(`${game}/${world}`)),
    deleteUnsavedFog: (game: string) => {
      for (const key of [...saved.fog.keys()]) if (key.startsWith(`${game}/unsaved:`)) saved.fog.delete(key)
      return Promise.resolve()
    },
    logUiError: () => Promise.resolve(),
  },
}))

import { emptyFog, encodeFog, reveal, saveFog } from '../lib/fog'
import type { Projection } from '../lib/projection'
import { discardUnsavedWorlds, enterWorld, loadFogFile } from './persist'
import { EMPTY_INDEX, EMPTY_WORLD_DATA, keys } from './worlds'

const p: Projection = { east: 'x', north: 'z', up: 'y', minEast: -400, maxEast: 400, minNorth: -400, maxNorth: 400 }
const WORLD = { id: 'sp:1', name: null, kind: 'single_player' as const }

function exploredFog() {
  const fog = emptyFog(p, 1)
  reveal(fog, 0, 0, 50)
  return fog
}

beforeEach(() => {
  saved.store.clear()
  saved.fog.clear()
  saved.failFogSaves = false
})

describe('enterWorld', () => {
  it('gives the first world what was explored and where the map was, pins or not', async () => {
    const fog = encodeFog(exploredFog())
    saved.fog.set('sotf/default', fog)
    saved.store.set(keys.session('sotf', 'default'), { layer: 'caves' })

    const index = await enterWorld('sotf', WORLD, EMPTY_INDEX)

    expect(index.claimed).toBe(true)
    expect(saved.fog.get('sotf/sp:1')).toEqual(fog)
    expect(saved.fog.has('sotf/default')).toBe(false)
    expect(saved.store.get(keys.session('sotf', 'sp:1'))).toEqual({ layer: 'caves' })
    expect(saved.store.has(keys.session('sotf', 'default'))).toBe(false)
  })

  it('joins old pins with the ones a first save already has, and keeps its own map state', async () => {
    const pin = (id: string) => ({ id, name: id, category: 'custom', position: { x: 0, y: 0, z: 0 } })
    saved.store.set(keys.data('sotf', 'default'), { ...EMPTY_WORLD_DATA, customPins: [pin('old')], found: ['a'] })
    saved.store.set(keys.data('sotf', 'sp:1'), { ...EMPTY_WORLD_DATA, customPins: [pin('new')] })
    saved.store.set(keys.session('sotf', 'default'), { layer: 'caves' })
    saved.store.set(keys.session('sotf', 'sp:1'), { layer: 'surface' })
    const own = encodeFog(emptyFog(p, 1))
    saved.fog.set('sotf/default', encodeFog(exploredFog()))
    saved.fog.set('sotf/sp:1', own)

    await enterWorld('sotf', WORLD, EMPTY_INDEX)

    const data = saved.store.get(keys.data('sotf', 'sp:1')) as typeof EMPTY_WORLD_DATA
    expect(data.customPins.map((c) => c.id)).toEqual(['new', 'old'])
    expect(data.found).toEqual(['a'])
    expect(saved.store.get(keys.data('sotf', 'default'))).toEqual(EMPTY_WORLD_DATA)
    expect(saved.store.get(keys.session('sotf', 'sp:1'))).toEqual({ layer: 'surface' })
    expect(saved.fog.get('sotf/sp:1')).toEqual(own)
  })

  it('moves fog an older version saved, even if it was never loaded since', async () => {
    const fog = exploredFog()
    saved.store.set(keys.fog('sotf', 'default'), saveFog(fog))

    await enterWorld('sotf', WORLD, EMPTY_INDEX)

    expect(saved.fog.get('sotf/sp:1')).toEqual(encodeFog(fog))
    expect(saved.store.has(keys.fog('sotf', 'default'))).toBe(false)
  })

  it('moves nothing into later worlds', async () => {
    saved.fog.set('sotf/default', encodeFog(exploredFog()))
    await enterWorld('sotf', WORLD, { ...EMPTY_INDEX, claimed: true })
    expect(saved.fog.has('sotf/sp:1')).toBe(false)
  })
})

describe('loadFogFile', () => {
  it('moves fog an older version saved to a file, once', async () => {
    const fog = exploredFog()
    saved.store.set(keys.fog('sotf', 'sp:1'), saveFog(fog))

    expect(await loadFogFile('sotf', 'sp:1', false)).toEqual(encodeFog(fog))
    expect(saved.fog.size).toBe(0)

    expect(await loadFogFile('sotf', 'sp:1', true)).toEqual(encodeFog(fog))
    expect(saved.fog.get('sotf/sp:1')).toEqual(encodeFog(fog))
    expect(saved.store.has(keys.fog('sotf', 'sp:1'))).toBe(false)
  })

  it('keeps the old copy when the file could not be written', async () => {
    saved.store.set(keys.fog('sotf', 'sp:1'), saveFog(exploredFog()))
    saved.failFogSaves = true
    await expect(loadFogFile('sotf', 'sp:1', true)).rejects.toThrow('disk full')
    expect(saved.store.has(keys.fog('sotf', 'sp:1'))).toBe(true)
  })

  it('has nothing for a world never explored', async () => {
    expect(await loadFogFile('sotf', 'sp:9', true)).toBeNull()
  })
})

describe('discardUnsavedWorlds', () => {
  it('forgets new games that were never saved, and nothing else', async () => {
    saved.store.set(keys.data('sotf', 'unsaved:1-1'), EMPTY_WORLD_DATA)
    saved.store.set(keys.session('sotf', 'unsaved:1-1'), {})
    saved.store.set(keys.fog('sotf', 'unsaved:1-1'), saveFog(exploredFog()))
    saved.store.set(keys.data('sotf', 'sp:1'), EMPTY_WORLD_DATA)
    saved.fog.set('sotf/unsaved:2-1', encodeFog(exploredFog()))
    saved.fog.set('sotf/sp:1', encodeFog(exploredFog()))

    await discardUnsavedWorlds('sotf')

    expect([...saved.store.keys()]).toEqual([keys.data('sotf', 'sp:1')])
    expect([...saved.fog.keys()]).toEqual(['sotf/sp:1'])
  })
})
