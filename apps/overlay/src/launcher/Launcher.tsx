// The launcher window: a compact list of supported games (status and main actions,
// details on demand), updates, and the settings. The map itself is the overlay.

import { getVersion } from '@tauri-apps/api/app'
import { AlertTriangle, ArrowLeft, ChevronDown, Download, Loader2, Map as MapIcon, Play, Plug, Search, Settings, SlidersHorizontal, Unplug, Zap } from 'lucide-react'
import { useCallback, useEffect, useState, type MouseEvent, type ReactNode } from 'react'
import { startAgentLogBuffer } from '../lib/agentLogs'
import { backend, events, type GameEntry, type PackStatus, type SetupChecks, type UpdateState } from '../lib/backend'
import type { LinkStatus } from '../lib/protocol'
import { SettingsView } from '../settings/SettingsView'
import { mapHotkey, useSettings } from '../settings/store'
import { startSettings } from '../settings/sync'
import { Button, IconButton, Segmented } from '../ui/controls'
import { filterGames, gameView, packView, updateView, type GameFilter } from './view'

function useLauncherData() {
  const games = useSettings((s) => s.games)
  const [status, setStatus] = useState<LinkStatus | null>(null)
  const [packs, setPacks] = useState<PackStatus[]>([])
  const [update, setUpdate] = useState<UpdateState | null>(null)
  const [version, setVersion] = useState('')

  useEffect(() => {
    const unlisten = [
      startSettings(),
      startAgentLogBuffer(),
      events.status(setStatus),
      events.mapPacks(setPacks),
      events.updateState(setUpdate),
    ]
    void (async () => {
      const [s, p, u, v] = await Promise.all([backend.status(), backend.mapPacks(), backend.updateState(), getVersion()])
      setStatus(s)
      setPacks(p)
      setUpdate(u)
      setVersion(v)
    })()
    return () => unlisten.forEach((p) => void p.then((f) => f()))
  }, [])

  return { games, status, packs, update, version }
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
  return { error, setError, run }
}

export function Launcher() {
  const data = useLauncherData()
  const { error, setError, run } = useAction()
  const autoUpdate = useSettings((s) => s.app?.auto_update ?? true)
  // Settings open on the game they were asked for, or the one running.
  const [settings, setSettings] = useState<{ gameId: string | null } | null>(null)
  const onSettingsError = useCallback((message: string) => setError(message), [setError])

  return (
    <div className="mx-auto flex h-full max-w-2xl flex-col gap-3 px-4 pb-4 pt-3">
      <header className="flex items-center gap-2.5">
        <img src="/logo.svg" alt="" className="size-7" />
        <h1 className="font-display text-[16px] font-semibold">Game Maps</h1>
        <span className="text-[11px] text-ink-faint">{data.version && `v${data.version}`}</span>
        {settings ? (
          <IconButton label="Back to games" className="ml-auto" onClick={() => setSettings(null)}>
            <ArrowLeft className="size-4" />
          </IconButton>
        ) : (
          <IconButton label="Settings" className="ml-auto" onClick={() => setSettings({ gameId: data.status?.game_id ?? null })}>
            <Settings className="size-4" />
          </IconButton>
        )}
      </header>

      <UpdateBanner update={data.update} autoUpdate={autoUpdate} onAction={run} />
      {error && <p className="rounded-lg bg-danger/15 px-3 py-2 text-[13px] text-danger">{error}</p>}

      {settings ? (
        <div className="min-h-0 flex-1 overflow-hidden rounded-xl border border-line bg-panel-raised">
          <SettingsView window="launcher" initialGameId={settings.gameId} onError={onSettingsError} />
        </div>
      ) : (
        <GameList data={data} onAction={run} onSettings={(gameId) => setSettings({ gameId })} />
      )}
    </div>
  )
}

function GameList({
  data,
  onAction,
  onSettings,
}: {
  data: ReturnType<typeof useLauncherData>
  onAction: (a: () => Promise<unknown>) => void
  onSettings: (gameId: string) => void
}) {
  const [query, setQuery] = useState('')
  const [filter, setFilter] = useState<GameFilter>('all')
  const games = filterGames(data.games, query, filter)

  return (
    <>
      <div className="flex items-center gap-2">
        <label className="flex min-w-0 flex-1 items-center gap-2 rounded-lg bg-panel-raised px-2.5 py-1.5 focus-within:ring-1 focus-within:ring-accent/60">
          <Search className="size-3.5 shrink-0 text-ink-faint" />
          <input
            type="search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search games"
            aria-label="Search games"
            className="min-w-0 flex-1 bg-transparent text-[13px] outline-none placeholder:text-ink-faint"
          />
        </label>
        <Segmented
          label="Show games"
          value={filter}
          options={[
            { id: 'all', label: 'All' },
            { id: 'live', label: 'Live', title: 'Games with live location and teleport' },
            { id: 'map', label: 'Map only', title: 'Games with a map but no live features yet' },
          ]}
          onChange={setFilter}
        />
      </div>

      <div className="scroll-thin -mx-1 flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto px-1">
        {games.map((game) => (
          <GameRow
            key={game.id}
            game={game}
            status={data.status}
            pack={data.packs.find((p) => p.game_id === game.id) ?? null}
            onSettings={() => onSettings(game.id)}
            onAction={onAction}
          />
        ))}
        {data.games.length > 0 && games.length === 0 && <p className="py-6 text-center text-[13px] text-ink-faint">No games match.</p>}
      </div>

      <p className="pt-1 text-center text-[11px] text-ink-faint">Closing this window keeps Game Maps running in the tray.</p>
    </>
  )
}

function SectionLabel({ children }: { children: ReactNode }) {
  return <h3 className="mb-1.5 text-[11px] font-semibold uppercase tracking-wider text-ink-faint">{children}</h3>
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
    <div className={`flex items-center gap-3 rounded-xl px-3.5 py-2.5 text-[13px] ${view.tone === 'error' ? 'bg-danger/15 text-danger' : 'bg-accent/15 text-ink'}`}>
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

/** One game: status and main actions on one line; options expand below. */
function GameRow({
  game,
  status,
  pack,
  onSettings,
  onAction,
}: {
  game: GameEntry
  status: LinkStatus | null
  pack: PackStatus | null
  /** Opens the settings on this game. */
  onSettings: () => void
  onAction: (a: () => Promise<unknown>) => void
}) {
  const [open, setOpen] = useState(false)
  const hookMode = useSettings((s) => s.app?.hook_modes[game.id] ?? 'auto')
  const mapKey = useSettings((s) => mapHotkey(s.app, game))
  const view = gameView(game, status, hookMode, mapKey)
  const checks = useSetupChecks(view.running)
  const map = pack && packView(pack)
  const detailsId = `game-${game.id}-options`

  // A click anywhere on the card that isn't on a control opens or closes the options.
  // Keyboard and screen reader users get the same through the arrow button.
  const onCardClick = (e: MouseEvent) => {
    if ((e.target as HTMLElement).closest(INTERACTIVE) || window.getSelection()?.toString()) return
    setOpen(!open)
  }

  return (
    <section className="cursor-pointer rounded-xl border border-line bg-panel-raised transition-colors hover:bg-white/[0.08]" onClick={onCardClick}>
      <div className="flex items-center gap-1.5 py-2 pl-3.5 pr-2">
        <div className="flex min-w-0 flex-1 items-center gap-2.5">
          <span className={`size-2 shrink-0 rounded-full ${TONES[view.tone]}`} aria-hidden />
          <span className="min-w-0">
            <span className="flex items-center gap-1.5">
              <span className="truncate font-display text-[14.5px] font-semibold">{game.name}</span>
              {game.liveFeatures && (
                <span
                  title="Live location and teleport"
                  className="inline-flex shrink-0 items-center gap-0.5 rounded bg-live/15 px-1 py-px text-[10px] font-semibold text-live"
                >
                  <Zap className="size-2.5" /> LIVE
                </span>
              )}
            </span>
            <span className="block truncate text-[12px] text-ink-dim">{view.text}</span>
          </span>
        </div>
        {!view.running && game.steamAppId !== null && (
          <Button tone="primary" className="px-2.5 py-1" onClick={() => onAction(() => backend.launchGame(game.id))}>
            <Play className="size-3.5" /> Play
          </Button>
        )}
        {view.canConnect && (
          <Button tone="primary" className="px-2.5 py-1" onClick={() => onAction(backend.connectGame)}>
            <Plug className="size-3.5" /> Connect
          </Button>
        )}
        {view.canDisconnect && (
          <IconButton label="Disconnect" onClick={() => onAction(backend.disconnectGame)}>
            <Unplug className="size-4" />
          </IconButton>
        )}
        <IconButton label="Open the map" onClick={() => onAction(backend.openMap)}>
          <MapIcon className="size-4" />
        </IconButton>
        <IconButton label={open ? 'Hide options' : 'Show options'} aria-expanded={open} aria-controls={detailsId} onClick={() => setOpen(!open)}>
          <ChevronDown className={`size-4 transition-transform ${open ? 'rotate-180' : ''}`} />
        </IconButton>
      </div>

      {/* Problems stay visible while collapsed. */}
      {(checks?.needs_admin || checks?.exclusive_fullscreen) && (
        <div className="px-3.5 pb-2.5">
          {checks.needs_admin && (
            <Warning>
              {game.name} is running as administrator, so Game Maps can't connect to it. Quit Game Maps from the tray, then
              right-click it and choose <b>Run as administrator</b>.
            </Warning>
          )}
          {checks.exclusive_fullscreen && (
            <Warning>
              {game.name} is in exclusive fullscreen, which hides the map. In the game's display settings, set the display
              mode to <b>Fullscreen Window</b> (borderless).
            </Warning>
          )}
        </div>
      )}

      {open && (
        <div id={detailsId} className="grid gap-3 border-t border-line px-3.5 pb-3 pt-2.5">
          {view.hint && <p className="text-[12px] text-ink-dim">{view.hint}</p>}
          <div>
            <SectionLabel>Map data</SectionLabel>
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
          <div>
            <Button onClick={onSettings}>
              <SlidersHorizontal className="size-3.5" /> Settings for {game.name}
            </Button>
          </div>
        </div>
      )}
    </section>
  )
}

function Warning({ children }: { children: ReactNode }) {
  return (
    <p className="mt-1.5 flex gap-2 rounded-lg bg-amber-300/12 px-3 py-2 text-[12.5px] text-amber-200">
      <AlertTriangle className="mt-0.5 size-4 shrink-0" />
      <span>{children}</span>
    </p>
  )
}

/** Elements inside a game card that do something themselves (no open/close). */
const INTERACTIVE = 'button, a, input, select, textarea, label, [role="radio"], [role="switch"]'

const TONES = {
  idle: 'bg-white/30',
  busy: 'bg-amber-300 animate-pulse',
  ok: 'bg-live',
  error: 'bg-danger',
} as const
