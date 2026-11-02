// The launcher window: supported games with their connection and map status, plus
// app settings and updates. Kept deliberately small; the map itself is the overlay.

import { getVersion } from '@tauri-apps/api/app'
import { AlertTriangle, Download, FolderOpen, Loader2, Map as MapIcon, Play, Plug, RefreshCw, Unplug } from 'lucide-react'
import { useEffect, useState, type ReactNode } from 'react'
import {
  backend,
  events,
  type AppSettings,
  type GameEntry,
  type HookMode,
  type PackStatus,
  type SetupChecks,
  type UpdateState,
} from '../lib/backend'
import type { Hotkey, LinkStatus } from '../lib/protocol'
import { HotkeyInput } from '../ui/HotkeyInput'
import { Button, Toggle } from '../ui/controls'
import { gameView, packView, updateView } from './view'

function useLauncherData() {
  const [games, setGames] = useState<GameEntry[]>([])
  const [status, setStatus] = useState<LinkStatus | null>(null)
  const [packs, setPacks] = useState<PackStatus[]>([])
  const [update, setUpdate] = useState<UpdateState | null>(null)
  const [settings, setSettings] = useState<AppSettings | null>(null)
  const [autostart, setAutostart] = useState(false)
  const [version, setVersion] = useState('')

  useEffect(() => {
    const unlisten = [
      events.status(setStatus),
      events.mapPacks(setPacks),
      events.updateState(setUpdate),
      // Settings can also change from the map (e.g. its hotkey).
      events.appSettings(setSettings),
    ]
    void (async () => {
      const [g, s, p, u, a, auto, v] = await Promise.all([
        backend.listGames(),
        backend.status(),
        backend.mapPacks(),
        backend.updateState(),
        backend.appSettings(),
        backend.autostart(),
        getVersion(),
      ])
      setGames(g)
      setStatus(s)
      setPacks(p)
      setUpdate(u)
      setSettings(a)
      setAutostart(auto)
      setVersion(v)
    })()
    return () => unlisten.forEach((p) => void p.then((f) => f()))
  }, [])

  return { games, status, packs, update, settings, setSettings, autostart, setAutostart, version }
}

/** Setup checks for the running game; Windows only reports some of them over time. */
function useSetupChecks(running: boolean) {
  const [checks, setChecks] = useState<SetupChecks | null>(null)
  useEffect(() => {
    if (!running) return
    const poll = () => void backend.setupChecks().then(setChecks)
    poll()
    const id = setInterval(poll, 2000)
    return () => clearInterval(id)
  }, [running])
  return running ? checks : null
}

/** Runs an action, showing its error (commands reject with a message string). */
function useAction() {
  const [error, setError] = useState<string | null>(null)
  const run = (action: () => Promise<unknown>) => {
    setError(null)
    action().catch((e: unknown) => setError(String(e)))
  }
  return { error, run }
}

export function Launcher() {
  const data = useLauncherData()
  const { error, run } = useAction()

  return (
    <div className="mx-auto flex min-h-full max-w-2xl flex-col gap-4 px-5 pb-5 pt-4">
      <header className="flex items-center gap-3">
        <img src="/favicon.svg" alt="" className="size-9" />
        <div className="min-w-0 flex-1">
          <h1 className="font-display text-[19px] font-semibold leading-tight">Game Maps</h1>
          <p className="text-[12px] text-ink-faint">{data.version && `Version ${data.version}`}</p>
        </div>
      </header>

      <UpdateBanner update={data.update} autoUpdate={data.settings?.auto_update ?? true} onAction={run} />

      {data.games.map((game) => (
        <GameCard
          key={game.id}
          game={game}
          status={data.status}
          pack={data.packs.find((p) => p.game_id === game.id) ?? null}
          hookMode={data.settings?.hook_modes[game.id] ?? 'auto'}
          onHookMode={(mode) => run(async () => data.setSettings(await backend.setHookMode(game.id, mode)))}
          hotkey={data.settings?.map_hotkeys[game.id] ?? null}
          onHotkey={(hotkey) => run(async () => data.setSettings(await backend.setMapHotkey(game.id, hotkey)))}
          onAction={run}
        />
      ))}

      {error && <p className="rounded-lg bg-danger/15 px-3 py-2 text-[13px] text-danger">{error}</p>}

      <Card>
        <h2 className="mb-1 text-[11px] font-semibold uppercase tracking-wider text-ink-faint">Settings</h2>
        <Toggle
          label="Start with Windows"
          hint="Runs quietly in the tray, so the map is ready whenever you play"
          checked={data.autostart}
          onChange={(enabled) => run(async () => data.setAutostart(await backend.setAutostart(enabled)))}
        />
        <Toggle
          label="Install updates automatically"
          hint="Updates install while no game is running"
          checked={data.settings?.auto_update ?? true}
          onChange={(enabled) => run(async () => data.setSettings(await backend.setAutoUpdate(enabled)))}
        />
        <div className="mt-2 flex flex-wrap gap-2">
          <Button
            disabled={data.update?.state === 'disabled' || data.update?.state === 'checking'}
            onClick={() => run(backend.checkForUpdates)}
          >
            <RefreshCw className="size-3.5" /> Check for updates
          </Button>
          <Button onClick={() => run(backend.openLogsFolder)}>
            <FolderOpen className="size-3.5" /> Open logs
          </Button>
        </div>
      </Card>

      <p className="mt-auto text-center text-[11px] text-ink-faint">
        Closing this window keeps Game Maps running in the tray. Quit from the tray icon.
      </p>
    </div>
  )
}

function Card({ children }: { children: ReactNode }) {
  return <section className="rounded-2xl border border-line bg-panel-raised p-4">{children}</section>
}

function UpdateBanner({
  update,
  autoUpdate,
  onAction,
}: {
  update: UpdateState | null
  autoUpdate: boolean
  onAction: (a: () => Promise<unknown>) => void
}) {
  const view = update && updateView(update, autoUpdate)
  if (!view) return null
  return (
    <div className={`flex items-center gap-3 rounded-xl px-4 py-3 text-[13px] ${view.tone === 'error' ? 'bg-danger/15 text-danger' : 'bg-accent/15 text-ink'}`}>
      {view.busy ? <Loader2 className="size-4 shrink-0 animate-spin" /> : <Download className="size-4 shrink-0" />}
      <span className="min-w-0 flex-1">{view.text}</span>
      {view.canInstall && (
        <Button tone="primary" onClick={() => onAction(backend.installUpdate)}>
          Update now
        </Button>
      )}
    </div>
  )
}

function GameCard({
  game,
  status,
  pack,
  hookMode,
  onHookMode,
  hotkey,
  onHotkey,
  onAction,
}: {
  game: GameEntry
  status: LinkStatus | null
  pack: PackStatus | null
  hookMode: HookMode
  onHookMode: (mode: HookMode) => void
  /** The user's key for this game; null uses the game's default. */
  hotkey: Hotkey | null
  onHotkey: (hotkey: Hotkey | null) => void
  onAction: (a: () => Promise<unknown>) => void
}) {
  const mapKey = hotkey ?? game.defaultHotkeys[0] ?? null
  const view = gameView(game, status, hookMode, mapKey)
  const checks = useSetupChecks(view.running)
  const map = pack && packView(pack)

  return (
    <Card>
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <h2 className="font-display text-[17px] font-semibold">{game.name}</h2>
          <p className="mt-0.5 flex items-center gap-2 text-[13px] text-ink-dim">
            <span className={`size-2 shrink-0 rounded-full ${TONES[view.tone]}`} />
            {view.text}
          </p>
        </div>
        <div className="flex shrink-0 flex-wrap justify-end gap-2">
          {!view.running && game.steamAppId !== null && (
            <Button tone="primary" onClick={() => onAction(() => backend.launchGame(game.id))}>
              <Play className="size-3.5" /> Play
            </Button>
          )}
          {view.canConnect && (
            <Button tone="primary" onClick={() => onAction(backend.connectGame)}>
              <Plug className="size-3.5" /> Connect
            </Button>
          )}
          {view.canDisconnect && (
            <Button onClick={() => onAction(backend.disconnectGame)}>
              <Unplug className="size-3.5" /> Disconnect
            </Button>
          )}
          <Button onClick={() => onAction(backend.openMap)} title="Open the map without the game">
            <MapIcon className="size-3.5" /> Map
          </Button>
        </div>
      </div>

      {view.hint && <p className="mt-2 text-[12px] text-ink-faint">{view.hint}</p>}

      {checks?.needs_admin && (
        <Warning>
          {game.name} is running as administrator, so Game Maps can't connect to it. Quit Game Maps from the tray, then
          right-click it and choose <b>Run as administrator</b>.
        </Warning>
      )}
      {checks?.exclusive_fullscreen && (
        <Warning>
          {game.name} is in exclusive fullscreen, which hides the map. In the game's display settings, set the display
          mode to <b>Fullscreen Window</b> (borderless).
        </Warning>
      )}

      <div className="mt-4 grid gap-3 border-t border-line pt-3 sm:grid-cols-2">
        <div>
          <h3 className="mb-1.5 text-[11px] font-semibold uppercase tracking-wider text-ink-faint">Connect to game</h3>
          <div className="inline-flex rounded-lg bg-black/30 p-0.5" role="radiogroup" aria-label="Connect to game">
            {(['auto', 'manual'] as const).map((mode) => (
              <button
                key={mode}
                type="button"
                role="radio"
                aria-checked={hookMode === mode}
                className={`rounded-md px-3 py-1 text-[12.5px] font-semibold transition-colors ${
                  hookMode === mode ? 'bg-accent text-accent-ink' : 'text-ink-dim hover:text-ink'
                }`}
                onClick={() => onHookMode(mode)}
              >
                {mode === 'auto' ? 'Automatically' : 'Manually'}
              </button>
            ))}
          </div>
          <p className="mt-1 text-[11px] text-ink-faint">
            {hookMode === 'auto' ? 'Connects as soon as the game starts.' : 'Connects only when you click Connect.'}
          </p>
        </div>

        <div>
          <h3 className="mb-1.5 text-[11px] font-semibold uppercase tracking-wider text-ink-faint">Key to open the map</h3>
          <HotkeyInput value={mapKey} isDefault={hotkey === null} onChange={onHotkey} />
          <p className="mt-1 text-[11px] text-ink-faint">Also changeable in the map's settings.</p>
        </div>

        <div>
          <h3 className="mb-1.5 text-[11px] font-semibold uppercase tracking-wider text-ink-faint">Map data</h3>
          {map && (
            <>
              <p className={`text-[12.5px] ${map.tone === 'error' ? 'text-danger' : 'text-ink-dim'}`}>{map.text}</p>
              {map.progress !== null && (
                <div className="mt-1.5 h-1.5 overflow-hidden rounded-full bg-black/40">
                  <div className="h-full rounded-full bg-accent transition-[width]" style={{ width: `${map.progress * 100}%` }} />
                </div>
              )}
              {map.action && (
                <Button className="mt-1.5" onClick={() => onAction(() => backend.installMapPack(game.id))}>
                  <Download className="size-3.5" /> {map.action}
                </Button>
              )}
            </>
          )}
        </div>
      </div>
    </Card>
  )
}

function Warning({ children }: { children: ReactNode }) {
  return (
    <p className="mt-3 flex gap-2 rounded-lg bg-amber-300/12 px-3 py-2 text-[12.5px] text-amber-200">
      <AlertTriangle className="mt-0.5 size-4 shrink-0" />
      <span>{children}</span>
    </p>
  )
}

const TONES = {
  idle: 'bg-white/30',
  busy: 'bg-amber-300 animate-pulse',
  ok: 'bg-live',
  error: 'bg-danger',
} as const
