// Plain-language launcher states, derived from backend status.

import type { GameEntry, HookMode, PackStatus, UpdateState } from '../lib/backend'
import { hotkeyLabel } from '../lib/keys'
import type { Hotkey, LinkStatus } from '../lib/protocol'

export type Tone = 'idle' | 'busy' | 'ok' | 'error'

export interface GameView {
  running: boolean
  tone: Tone
  text: string
  hint: string | null
  canConnect: boolean
  canDisconnect: boolean
}

export function gameView(game: GameEntry, status: LinkStatus | null, hookMode: HookMode, mapKey: Hotkey | null = null): GameView {
  const base = { running: false, tone: 'idle' as Tone, hint: null, canConnect: false, canDisconnect: false }
  if (!status || status.game_id !== game.id || status.state === 'waiting_for_game') {
    return {
      ...base,
      text: 'Not running',
      hint: hookMode === 'auto' ? 'Start the game and Game Maps connects on its own.' : 'Start the game, then click Connect.',
    }
  }
  const running = { ...base, running: true }
  switch (status.state) {
    case 'ready':
      return {
        ...running,
        text: status.message === 'Disconnected' ? 'Running, disconnected' : 'Running, not connected',
        canConnect: true,
      }
    case 'connecting':
      return { ...running, tone: 'busy', text: 'Connecting…', hint: 'This can take a minute while the game loads.', canDisconnect: true }
    case 'connected':
      return {
        ...running,
        tone: 'ok',
        text: status.message ?? 'Connected',
        hint: mapKey ? `Press ${hotkeyLabel(mapKey)} in the game to open the map.` : 'Set a key below to open the map in game.',
        canDisconnect: true,
      }
    case 'error':
      return { ...running, tone: 'error', text: status.message ?? 'Could not connect', canConnect: true }
  }
}

export type GameFilter = 'all' | 'live' | 'map'

/** Games matching a search and a filter (`live`: live location and teleport, `map`: map only). */
export function filterGames(games: GameEntry[], query: string, filter: GameFilter): GameEntry[] {
  const q = query.trim().toLowerCase()
  return games.filter(
    (g) => (filter === 'all' || g.liveFeatures === (filter === 'live')) && (!q || g.name.toLowerCase().includes(q)),
  )
}

export interface PackView {
  tone: 'idle' | 'error'
  text: string
  /** 0..1 while downloading. */
  progress: number | null
  /** Button label, when the user can download or update. */
  action: string | null
}

export function formatSize(bytes: number): string {
  return bytes >= 1024 * 1024 ? `${(bytes / (1024 * 1024)).toFixed(0)} MB` : `${Math.max(1, Math.round(bytes / 1024))} KB`
}

export function packView(pack: PackStatus): PackView {
  if (pack.downloading) {
    const { received, total } = pack.downloading
    const progress = total > 0 ? Math.min(1, received / total) : 0
    return { tone: 'idle', text: `Downloading… ${Math.round(progress * 100)}%`, progress, action: null }
  }
  const retry = pack.available ? (pack.installed ? 'Retry update' : 'Retry download') : null
  if (pack.error) return { tone: 'error', text: pack.error, progress: null, action: retry }
  if (pack.installed === 'development') return { tone: 'idle', text: 'Using local development data', progress: null, action: null }
  if (pack.installed && pack.available && pack.available.version !== pack.installed) {
    return { tone: 'idle', text: `Update available (${formatSize(pack.available.size)})`, progress: null, action: 'Update map' }
  }
  if (pack.installed) return { tone: 'idle', text: `Ready (version ${pack.installed})`, progress: null, action: null }
  if (pack.available) {
    return { tone: 'idle', text: `Not downloaded (${formatSize(pack.available.size)}). It downloads when you play, or now`, progress: null, action: 'Download now' }
  }
  return { tone: 'idle', text: 'No map published yet, or you are offline. Game Maps keeps checking.', progress: null, action: null }
}

export interface UpdateView {
  tone: 'info' | 'error'
  text: string
  busy: boolean
  canInstall: boolean
}

/** Banner for app updates; null when there is nothing to say. */
export function updateView(update: UpdateState, autoUpdate: boolean): UpdateView | null {
  switch (update.state) {
    case 'available':
      return {
        tone: 'info',
        text: autoUpdate
          ? `Version ${update.version} is ready. It installs when no game is running.`
          : `Version ${update.version} is available.`,
        busy: false,
        canInstall: true,
      }
    case 'downloading': {
      const pct = update.total ? ` ${Math.round((update.received / update.total) * 100)}%` : ''
      return { tone: 'info', text: `Downloading update…${pct}`, busy: true, canInstall: false }
    }
    case 'installing':
      return { tone: 'info', text: 'Installing update. Game Maps will restart.', busy: true, canInstall: false }
    case 'checking':
      return { tone: 'info', text: 'Checking for updates…', busy: true, canInstall: false }
    case 'error':
      return { tone: 'error', text: update.message, busy: false, canInstall: false }
    default:
      return null
  }
}
