import { describe, expect, it } from 'vitest'
import type { GameEntry, PackStatus } from '../lib/backend'
import type { LinkStatus } from '../lib/protocol'
import { filterGames, formatSize, gameView, packView, updateView } from './view'

const game: GameEntry = { id: 'sotf', name: 'Sons of the Forest', process: 'x.exe', steamAppId: 1, defaultHotkeys: [], liveFeatures: true }
const status = (s: Partial<LinkStatus>): LinkStatus => ({
  state: 'waiting_for_game',
  game_id: 'sotf',
  pid: 1,
  capabilities: null,
  message: null,
  ...s,
})

describe('gameView', () => {
  it('is not running when another or no game is detected', () => {
    expect(gameView(game, null, 'auto').running).toBe(false)
    expect(gameView(game, status({ state: 'connected', game_id: 'other' }), 'auto').running).toBe(false)
  })

  it('offers connect when ready or failed, disconnect when connected', () => {
    expect(gameView(game, status({ state: 'ready' }), 'manual')).toMatchObject({ running: true, canConnect: true, canDisconnect: false })
    expect(gameView(game, status({ state: 'error', message: 'boom' }), 'auto')).toMatchObject({ canConnect: true, text: 'boom' })
    expect(gameView(game, status({ state: 'connected' }), 'auto')).toMatchObject({ tone: 'ok', canDisconnect: true, canConnect: false })
  })

  it('names the map key when connected', () => {
    const key = { action: 'toggle_map' as const, vk: 0x4b, ctrl: true }
    expect(gameView(game, status({ state: 'connected' }), 'auto', key).hint).toBe('Press Ctrl + K in the game to open the map.')
  })

  it('explains a user disconnect', () => {
    expect(gameView(game, status({ state: 'ready', message: 'Disconnected' }), 'auto').text).toBe('Running, disconnected')
  })
})

describe('packView', () => {
  const remote = { version: '2', url: '', sha256: '', size: 13 * 1024 * 1024 }
  const pack = (p: Partial<PackStatus>): PackStatus => ({ game_id: 'sotf', installed: null, available: null, downloading: null, error: null, ...p })

  it('offers a download before the first install', () => {
    expect(packView(pack({ available: remote }))).toMatchObject({ action: 'Download now' })
  })

  it('shows progress while downloading', () => {
    expect(packView(pack({ available: remote, downloading: { received: 50, total: 200 } }))).toMatchObject({ progress: 0.25, action: null })
  })

  it('offers updates only for a different version', () => {
    expect(packView(pack({ installed: '1', available: remote })).action).toBe('Update map')
    expect(packView(pack({ installed: '2', available: remote })).action).toBeNull()
    expect(packView(pack({ installed: 'development', available: remote })).action).toBeNull()
  })

  it('lets the user retry after an error', () => {
    expect(packView(pack({ available: remote, error: 'offline' }))).toMatchObject({ tone: 'error', action: 'Retry download' })
  })
})

describe('updateView', () => {
  it('hides quiet states', () => {
    expect(updateView({ state: 'up_to_date' }, true)).toBeNull()
    expect(updateView({ state: 'disabled' }, true)).toBeNull()
  })

  it('offers install when available', () => {
    expect(updateView({ state: 'available', version: '1.2.0', notes: null }, false)).toMatchObject({ canInstall: true })
  })
})

it('formats sizes', () => {
  expect(formatSize(13 * 1024 * 1024)).toBe('13 MB')
  expect(formatSize(100)).toBe('1 KB')
})

describe('filterGames', () => {
  const games: GameEntry[] = [game, { ...game, id: 'x', name: 'Another Island', liveFeatures: false }]
  const ids = (q: string, f: Parameters<typeof filterGames>[2]) => filterGames(games, q, f).map((g) => g.id)

  it('filters by live features', () => {
    expect(ids('', 'all')).toEqual(['sotf', 'x'])
    expect(ids('', 'live')).toEqual(['sotf'])
    expect(ids('', 'map')).toEqual(['x'])
  })

  it('searches names, ignoring case and spaces around the query', () => {
    expect(ids(' FOREST ', 'all')).toEqual(['sotf'])
    expect(ids('island', 'live')).toEqual([])
  })
})
