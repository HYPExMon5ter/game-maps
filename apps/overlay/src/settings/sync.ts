// Loads the settings into `useSettings`, saves the UI settings when they change, and
// keeps both windows in step: a change in one window is sent to the other, and the
// backend announces changes to its own settings.

import { emit, listen } from '@tauri-apps/api/event'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { backend, events } from '../lib/backend'
import type { Hotkey } from '../lib/protocol'
import { getStore } from '../state/persist'
import { readSettings, type SavedSettings, type Settings } from './model'
import { useSettings } from './store'

/** Window to window: the UI settings someone changed. */
const EVENT = 'ui-settings'
const STORE_KEY = 'settings'

interface Changed {
  from: string
  settings: Settings
}

/** `value` as text with object keys in order: what's saved doesn't keep theirs. */
function sortedJson(value: unknown): string {
  return JSON.stringify(value, (_, v: unknown) => (v && typeof v === 'object' && !Array.isArray(v) ? Object.fromEntries(Object.entries(v).sort(([a], [b]) => (a < b ? -1 : 1))) : v))
}

function sameJson(a: unknown, b: unknown): boolean {
  return sortedJson(a) === sortedJson(b)
}

/** Starts loading and syncing; resolves to a function that stops it. */
export async function startSettings(): Promise<() => void> {
  const self = getCurrentWebviewWindow().label
  const state = useSettings.getState()
  // Settings arriving from the other window are already saved; don't send them back.
  let applyingRemote = false
  // Newer than what's being loaded, if it comes in meanwhile.
  let received: Settings | null = null
  const unlisten = await Promise.all([
    events.appSettings(state.setApp),
    listen<Changed>(EVENT, ({ payload }) => {
      if (payload.from === self) return
      received = payload.settings
      applyingRemote = true
      useSettings.setState({ ui: payload.settings })
      applyingRemote = false
    }),
  ])

  const store = await getStore()
  const [saved, app, games, autostart] = await Promise.all([
    store.get<SavedSettings>(STORE_KEY),
    backend.appSettings(),
    backend.listGames(),
    backend.autostart(),
  ])
  const { settings, legacyHotkey } = readSettings(saved, games.map((g) => g.id))
  useSettings.setState({ ui: received ?? settings, app, games, autostart })
  // Older versions saved them differently (see `readSettings`).
  if (saved && !received && !sameJson(saved, settings)) await store.set(STORE_KEY, settings)
  if (legacyHotkey) void applyLegacyHotkey(legacyHotkey)

  const stop = useSettings.subscribe((s, prev) => {
    if (s.ui === prev.ui || applyingRemote) return
    void store.set(STORE_KEY, s.ui)
    void emit(EVENT, { from: self, settings: s.ui } satisfies Changed)
  })
  return () => {
    stop()
    unlisten.forEach((f) => f())
  }
}

/**
 * Older versions kept one map hotkey for all games in the UI settings: applies it to
 * every game without its own key.
 */
async function applyLegacyHotkey(hotkey: Hotkey) {
  const { games, setApp } = useSettings.getState()
  let latest = await backend.appSettings()
  for (const game of games) {
    if (!latest.map_hotkeys[game.id]) latest = await backend.setMapHotkey(game.id, hotkey)
  }
  setApp(latest)
}
