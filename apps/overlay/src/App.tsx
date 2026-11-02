import { Component, useEffect, useState, type ReactNode } from 'react'
import { startAgentLogBuffer } from './lib/agentLogs'
import { backend, events } from './lib/backend'
import { loadGame } from './lib/game'
import { matchesHotkey } from './lib/keys'
import { mapHandle } from './map/handle'
import { clearLegacyHotkey, loadSettings, loadUserData, startPersisting } from './state/persist'
import { selectMapHotkey, useStore } from './state/store'
import type { Hotkey } from './lib/protocol'
import { FullMap } from './ui/FullMap'
import { COMPASS_HEIGHT, Hud, HUD_GAP } from './ui/Hud'

/** A new map pack arrived while the full map was open. */
let reloadWhenClosed = false

function useBackendSync() {
  useEffect(() => {
    const { setStatus, setMode, setSnapshot, setSettings, setMapHotkeys } = useStore.getState()
    const cleanups: Promise<() => void>[] = [
      events.status(setStatus),
      // Hotkeys may be changed from the launcher too.
      events.appSettings((s) => setMapHotkeys(s.map_hotkeys)),
      events.snapshot((s) => setSnapshot(s.local || s.others.length ? s : null)),
      events.overlayMode(async (mode) => {
        setMode(mode)
        if (mode !== 'open' && reloadWhenClosed) return window.location.reload()
        // Snapshots are only streamed while visible; catch up immediately.
        if (mode !== 'hidden') setSnapshot(await backend.lastSnapshot())
        if (mode === 'open') requestAnimationFrame(() => mapHandle.current?.resize())
      }),
      startAgentLogBuffer(),
      // New map data for the shown game: reload so tiles, icons and pins all switch
      // over, but not while the user is looking at the map.
      events.mapPackInstalled((gameId) => {
        if (useStore.getState().game?.id !== gameId) return
        if (useStore.getState().mode === 'open') reloadWhenClosed = true
        else window.location.reload()
      }),
    ]
    let stopPersist: (() => void) | null = null
    void (async () => {
      const [{ settings, legacyHotkey }, appSettings] = await Promise.all([loadSettings(), backend.appSettings()])
      setSettings(settings)
      setMapHotkeys(appSettings.map_hotkeys)
      stopPersist = startPersisting()
      if (legacyHotkey) void migrateLegacyHotkey(legacyHotkey)
      setStatus(await backend.status())
      setMode(await backend.overlayMode())
      setSnapshot(await backend.lastSnapshot())
    })()
    return () => {
      stopPersist?.()
      cleanups.forEach((p) => void p.then((f) => f()))
    }
  }, [])
}

/** Loads the detected game, or the first available one so the map can be browsed offline. */
function useGameLoader() {
  const detected = useStore((s) => s.status?.game_id ?? null)
  const current = useStore((s) => s.game?.id ?? null)
  useEffect(() => {
    let cancelled = false
    void (async () => {
      const id = detected ?? current ?? (await backend.listGames())[0]?.id
      if (!id || id === current) return
      try {
        const [{ game, pins }, user] = await Promise.all([loadGame(id), loadUserData(id)])
        if (!cancelled) useStore.getState().setGame(game, pins, user)
      } catch (e) {
        useStore.getState().notify(`Could not load map: ${String(e)}`, 'error')
      }
    })()
    return () => {
      cancelled = true
    }
  }, [detected, current])
}

/**
 * Older versions kept one map hotkey for all games in the UI settings. Applies it to
 * every game without its own key, once (the UI settings no longer store it).
 */
async function migrateLegacyHotkey(hotkey: Hotkey) {
  const [games, current] = await Promise.all([backend.listGames(), backend.appSettings()])
  let latest = current
  for (const game of games) {
    if (!latest.map_hotkeys[game.id]) latest = await backend.setMapHotkey(game.id, hotkey)
  }
  useStore.getState().setMapHotkeys(latest.map_hotkeys)
  await clearLegacyHotkey()
}

/** Keeps the backend's HUD window geometry in line with what the HUD shows. */
function useHudSync() {
  const hud = useStore((s) => s.settings.hud)
  const tracking = useStore((s) => s.tracked !== null)
  useEffect(() => {
    const minimap = hud.minimap
    const width = minimap ? hud.size : 280
    const height = (minimap ? hud.size : 0) + (tracking ? COMPASS_HEIGHT + (minimap ? HUD_GAP : 0) : 0)
    void backend.setPinConfig({ enabled: minimap || tracking, corner: hud.corner, width, height: Math.max(height, 1), margin: 24 })
  }, [hud, tracking])
}

/** M (or the configured key) and Esc close the full map while it has focus. */
function useOverlayKeys() {
  const mode = useStore((s) => s.mode)
  useEffect(() => {
    if (mode !== 'open') return
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement
      if (target.closest('input, textarea, select, [contenteditable]')) return
      if (e.key === '/' || (e.ctrlKey && e.key.toLowerCase() === 'f')) {
        e.preventDefault()
        document.getElementById('pin-search')?.focus()
        return
      }
      const hotkey = selectMapHotkey(useStore.getState())
      if (e.key === 'Escape' || (hotkey && matchesHotkey(e, hotkey))) {
        e.preventDefault()
        void backend.closeOverlay()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [mode])
}

export default function App() {
  useBackendSync()
  useGameLoader()
  useHudSync()
  useOverlayKeys()
  const mode = useStore((s) => s.mode)
  const hasGame = useStore((s) => s.game !== null)
  // Mount the full map on first open, then keep it (laid out but invisible) so
  // reopening is instant. It must never be display:none: MapLibre needs a real size.
  const [fullMapMounted, setFullMapMounted] = useState(false)
  if (mode === 'open' && !fullMapMounted) setFullMapMounted(true)
  if (!hasGame) return null

  return (
    <ErrorBoundary>
      {fullMapMounted && (
        <div className={`absolute inset-0 ${mode === 'open' ? '' : 'invisible'}`} aria-hidden={mode !== 'open'}>
          <FullMap />
        </div>
      )}
      {mode === 'pinned' && (
        <div className="absolute inset-0">
          <Hud />
        </div>
      )}
    </ErrorBoundary>
  )
}

class ErrorBoundary extends Component<{ children: ReactNode }, { error: Error | null }> {
  state = { error: null as Error | null }

  static getDerivedStateFromError(error: Error) {
    return { error }
  }

  render() {
    if (!this.state.error) return this.props.children
    return (
      <div className="grid h-full place-items-center">
        <div className="glass max-w-md rounded-2xl p-5 text-[13px]">
          <h2 className="font-display mb-2 text-[16px] font-semibold">The map hit an error</h2>
          <p className="mb-3 break-words font-mono text-[12px] text-ink-dim">{this.state.error.message}</p>
          <button className="rounded-lg bg-accent px-3 py-1.5 font-semibold text-accent-ink" onClick={() => this.setState({ error: null })}>
            Try again
          </button>
        </div>
      </div>
    )
  }
}
