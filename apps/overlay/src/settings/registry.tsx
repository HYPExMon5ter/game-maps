// Every setting, where it's filed and how it's changed. The settings panel lists these
// by category and searches them, so a new setting only needs an entry here.
//
// `scope: 'game'` settings are kept per game (shown for the game picked in the panel);
// the rest apply to every game.
//
// The registry is data with its controls alongside; edits reload the page in development.
// oxlint-disable react/only-export-components

import { Download, ExternalLink, FileText, FolderOpen, Gamepad2, Star, Keyboard, Map as MapIcon, MapPinned, Move, Navigation, PictureInPicture2, RefreshCw, SlidersHorizontal, Users, Wrench, type LucideIcon } from 'lucide-react'
import { useCallback, useEffect, useRef, useState, type ComponentType, type ReactNode } from 'react'
import { agentLogs } from '../lib/agentLogs'
import { backend, events, type ActionKeys, type AppSettings, type GameEntry, type HookMode, type MonitorInfo, type PackStatus, type UpdateState, type ZoomKeys } from '../lib/backend'
import { packView } from '../launcher/view'
import { formatDistance } from '../lib/compass'
import type { AgentLog, Corner, Hotkey, HotkeyAction, LinkStatus } from '../lib/protocol'
import { exportPins, importPins, newPinsFrom } from '../lib/pinTransfer'
import { saveProblemReport } from '../lib/report'
import { readyPeers, useSyncStore } from '../lib/sync/sync'
import { MAX_UI_SCALE, MIN_UI_SCALE, useUiScale } from '../lib/uiScale'
import { ConnectionCheck } from '../ui/ConnectionCheck'
import { Button, Segmented, Slider, Toggle } from '../ui/controls'
import { HotkeyInput } from '../ui/HotkeyInput'
import { resetFog } from '../state/fog'
import { useStore } from '../state/store'
import { clearTrail } from '../state/trail'
import { cornerAnchor, gameSettings, type GameSettings, type HudSettings, type Settings } from './model'
import { hookMode, mapHotkey, setMinimapLayout, useGameHud, useSettings } from './store'

export type CategoryId = 'general' | 'keys' | 'map' | 'minimap' | 'content' | 'tracking' | 'multiplayer' | 'advanced'

export interface Category {
  id: CategoryId
  title: string
  icon: LucideIcon
}

export const CATEGORIES: Category[] = [
  { id: 'general', title: 'General', icon: SlidersHorizontal },
  { id: 'keys', title: 'Keys', icon: Keyboard },
  { id: 'map', title: 'Full map', icon: MapIcon },
  { id: 'minimap', title: 'Mini-map', icon: PictureInPicture2 },
  { id: 'content', title: 'Map content', icon: MapPinned },
  { id: 'tracking', title: 'Tracking', icon: Navigation },
  { id: 'multiplayer', title: 'Multiplayer', icon: Users },
  { id: 'advanced', title: 'Advanced', icon: Wrench },
]

/** Where the panel is shown, and for which game. */
export interface SettingsContext {
  /** The game per-game settings are shown for. */
  game: GameEntry | null
  /** The map window (with a game loaded) or the launcher. */
  window: 'map' | 'launcher'
  /** Shows an error from a backend call. */
  onError(message: string): void
}

export interface ControlProps {
  /** Name and description, with the per-game marker added for per-game settings. */
  label: ReactNode
  hint?: string
  ctx: SettingsContext
}

export interface SettingDef {
  id: string
  category: CategoryId
  /** Per game, or the same for every game. */
  scope: 'all' | 'game'
  label: string
  hint?: string
  /** Other words people might search for. */
  keywords?: string
  /** Hidden where it doesn't apply (e.g. a game without live features). */
  applies?: (ctx: SettingsContext) => boolean
  Control: ComponentType<ControlProps>
}

const pct = (v: number) => `${Math.round(v * 100)}%`
const live = (ctx: SettingsContext) => !!ctx.game?.liveFeatures

// Controls -------------------------------------------------------------------------

/** A switch for a setting for every game. */
function uiToggle(key: { [K in keyof Settings]: Settings[K] extends boolean ? K : never }[keyof Settings]): ComponentType<ControlProps> {
  return function UiToggle({ label, hint }) {
    const checked = useSettings((s) => s.ui[key])
    const setUi = useSettings((s) => s.setUi)
    return <Toggle label={label} hint={hint} checked={checked} onChange={(v) => setUi({ [key]: v })} />
  }
}

function hudToggle(key: { [K in keyof HudSettings]: HudSettings[K] extends boolean ? K : never }[keyof HudSettings]): ComponentType<ControlProps> {
  return function HudToggle({ label, hint }) {
    const checked = useSettings((s) => s.ui.hud[key])
    const setHud = useSettings((s) => s.setHud)
    return <Toggle label={label} hint={hint} checked={checked} onChange={(v) => setHud({ [key]: v })} />
  }
}

type GameFlag = { [K in keyof GameSettings]: GameSettings[K] extends boolean ? K : never }[keyof GameSettings]

/** A switch for a setting of the panel's game. */
function gameToggle(key: GameFlag, requires?: GameFlag): ComponentType<ControlProps> {
  return function GameToggle({ label, hint, ctx }) {
    const id = ctx.game?.id ?? null
    const checked = useSettings((s) => gameSettings(s.ui, id)[key])
    const enabled = useSettings((s) => !requires || gameSettings(s.ui, id)[requires])
    const setGameSettings = useSettings((s) => s.setGameSettings)
    if (!id) return null
    return <Toggle label={label} hint={hint} checked={checked} disabled={!enabled} onChange={(v) => setGameSettings(id, { [key]: v })} />
  }
}

function DangerDistance({ label, ctx }: ControlProps) {
  const id = ctx.game?.id ?? null
  const meters = useSettings((s) => gameSettings(s.ui, id).dangerMeters)
  const enabled = useSettings((s) => gameSettings(s.ui, id).dangerAlert)
  const units = useSettings((s) => s.ui.units)
  const setGameSettings = useSettings((s) => s.setGameSettings)
  if (!id || !enabled) return null
  return <Slider label={label} value={meters} min={10} max={100} step={5} format={(m) => formatDistance(m, units)} onChange={(dangerMeters) => setGameSettings(id, { dangerMeters })} />
}

function uiSlider(
  key: 'opacity' | 'pinSize',
  range: { min: number; max: number; step: number },
): ComponentType<ControlProps> {
  return function UiSlider({ label }) {
    const value = useSettings((s) => s.ui[key])
    const setUi = useSettings((s) => s.setUi)
    return <Slider label={label} value={value} {...range} format={pct} onChange={(v) => setUi({ [key]: v })} />
  }
}

/** Starts moving and resizing the map and mini-map (in the map, where you can see them). */
function EditLayout({ label, hint, ctx }: ControlProps) {
  if (ctx.window !== 'map') return null
  return (
    <Row label={label} hint={hint}>
      <Button onClick={() => useStore.getState().setEditingLayout(true)}>
        <Move className="size-4" /> Edit layout
      </Button>
    </Row>
  )
}

function Row({ label, hint, children }: { label: ReactNode; hint?: string; children: ReactNode }) {
  return (
    <div className="flex flex-wrap items-start justify-between gap-x-3 gap-y-1.5 py-1">
      <span className="min-w-[9rem] flex-1">
        <span className="block text-[13px]">{label}</span>
        {hint && <span className="block text-[11px] text-ink-faint">{hint}</span>}
      </span>
      <div className="ml-auto flex shrink-0 flex-col items-end gap-1.5">{children}</div>
    </div>
  )
}

/** Saves a backend setting, keeping the store in step and reporting failures. */
function useSaveApp(ctx: SettingsContext) {
  const setApp = useSettings((s) => s.setApp)
  return useCallback(
    (save: () => Promise<AppSettings>) =>
      save()
        .then(setApp)
        .catch((e: unknown) => ctx.onError(String(e))),
    [setApp, ctx],
  )
}

function Units({ label, hint }: ControlProps) {
  const metric = useSettings((s) => s.ui.units === 'metric')
  const setUi = useSettings((s) => s.setUi)
  return <Toggle label={label} hint={hint} checked={metric} onChange={(m) => setUi({ units: m ? 'metric' : 'imperial' })} />
}

function InterfaceSize({ label, hint }: ControlProps) {
  const manual = useSettings((s) => s.ui.uiScale)
  const setUi = useSettings((s) => s.setUi)
  const current = useUiScale()
  return (
    <>
      <Toggle label={label} hint={hint} checked={manual === null} onChange={(auto) => setUi({ uiScale: auto ? null : current })} />
      {manual !== null && (
        <Slider label="Interface size" value={manual} min={MIN_UI_SCALE} max={MAX_UI_SCALE} step={0.05} format={pct} onChange={(uiScale) => setUi({ uiScale })} />
      )}
    </>
  )
}

function Autostart({ label, hint, ctx }: ControlProps) {
  const enabled = useSettings((s) => s.autostart)
  const setAutostart = useSettings((s) => s.setAutostart)
  const set = (on: boolean) =>
    backend
      .setAutostart(on)
      .then(setAutostart)
      .catch((e: unknown) => ctx.onError(String(e)))
  return <Toggle label={label} hint={hint} checked={enabled ?? false} disabled={enabled === null} onChange={(on) => void set(on)} />
}

function useUpdateState() {
  const [state, setState] = useState<UpdateState | null>(null)
  useEffect(() => {
    const unlisten = events.updateState(setState)
    void backend.updateState().then(setState)
    return () => void unlisten.then((f) => f())
  }, [])
  return state
}

function Analytics({ label, hint, ctx }: ControlProps) {
  const enabled = useSettings((s) => s.app?.analytics ?? false)
  const save = useSaveApp(ctx)
  return <Toggle label={label} hint={hint} checked={enabled} onChange={(on) => void save(() => backend.setAnalytics(on))} />
}

function AutoUpdate({ label, hint, ctx }: ControlProps) {
  const enabled = useSettings((s) => s.app?.auto_update ?? true)
  const save = useSaveApp(ctx)
  const update = useUpdateState()
  return (
    <>
      <Toggle label={label} hint={hint} checked={enabled} onChange={(on) => void save(() => backend.setAutoUpdate(on))} />
      <div className="flex flex-wrap gap-2 pb-1">
        <Button
          disabled={update?.state === 'disabled' || update?.state === 'checking'}
          onClick={() => void backend.checkForUpdates().catch((e: unknown) => ctx.onError(String(e)))}
        >
          <RefreshCw className="size-3.5" /> Check for updates
        </Button>
        {update?.state === 'available' && (
          <Button tone="primary" onClick={() => void backend.installUpdate().catch((e: unknown) => ctx.onError(String(e)))}>
            <Download className="size-3.5" /> Update to {update.version}
          </Button>
        )}
      </div>
    </>
  )
}

function ConnectAll({ label, hint, ctx }: ControlProps) {
  const auto = useSettings((s) => s.app?.default_hook_mode !== 'manual')
  const save = useSaveApp(ctx)
  return <Toggle label={label} hint={hint} checked={auto} onChange={(on) => void save(() => backend.setAllHookModes(on ? 'auto' : 'manual'))} />
}

/** A game's map data, for the game the panel is on. */
function MapData({ label, ctx }: ControlProps) {
  const id = ctx.game?.id
  const [packs, setPacks] = useState<PackStatus[]>([])
  useEffect(() => {
    const unlisten = events.mapPacks(setPacks)
    void backend.mapPacks().then(setPacks)
    return () => void unlisten.then((f) => f())
  }, [])
  const pack = packs.find((p) => p.game_id === id)
  if (!id || !pack) return null
  const view = packView(pack)
  return (
    <Row label={label} hint={view.text}>
      {view.action && (
        <Button onClick={() => void backend.installMapPack(id).catch((e: unknown) => ctx.onError(String(e)))}>
          <Download className="size-3.5" /> {view.action}
        </Button>
      )}
    </Row>
  )
}

function StarProject({ label, hint, ctx }: ControlProps) {
  return (
    <Row label={label} hint={hint}>
      <Button onClick={() => void backend.openProjectPage().catch((e: unknown) => ctx.onError(String(e)))}>
        <Star className="size-3.5" /> Star on GitHub
      </Button>
    </Row>
  )
}

function ConnectMode({ label, hint, ctx }: ControlProps) {
  const id = ctx.game?.id
  const mode = useSettings((s) => hookMode(s.app, id))
  const save = useSaveApp(ctx)
  if (!id) return null
  return (
    <Row label={label} hint={hint}>
      <Segmented<HookMode>
        label="Connect to game"
        value={mode}
        options={[
          { id: 'auto', label: 'Automatically', title: 'Connects as soon as the game starts' },
          { id: 'manual', label: 'Manually', title: 'Connects only when you click Connect' },
        ]}
        onChange={(m) => void save(() => backend.setHookMode(id, m))}
      />
    </Row>
  )
}

function MapKey({ label, hint, ctx }: ControlProps) {
  const game = ctx.game
  const current = useSettings((s) => mapHotkey(s.app, game))
  const isDefault = useSettings((s) => !game || !s.app?.map_hotkeys[game.id])
  const save = useSaveApp(ctx)
  if (!game) return null
  const set = (hotkey: Hotkey | null) => void save(() => backend.setMapHotkey(game.id, hotkey))
  return (
    <Row label={label} hint={hint}>
      <HotkeyInput value={current} isDefault={isDefault} onChange={set} onReset={() => set(null)} />
    </Row>
  )
}

/** The key for something the map does itself. */
function actionKey(action: HotkeyAction & keyof ActionKeys): ComponentType<ControlProps> {
  return function ActionKey({ label, hint, ctx }) {
    const current = useSettings((s) => s.app?.action_keys[action] ?? null)
    const save = useSaveApp(ctx)
    const set = (key: Hotkey | null) => void save(() => backend.setActionKey(action, key))
    return (
      <Row label={label} hint={hint}>
        <HotkeyInput value={current} action={action} label={`${label} key`} onChange={set} onClear={() => set(null)} />
      </Row>
    )
  }
}

const pinCount = (n: number) => (n === 1 ? '1 pin' : `${n} pins`)

/** Saves this world's custom pins to a file, and adds pins from one. */
function PinFiles({ label, hint, ctx }: ControlProps) {
  const fileRef = useRef<HTMLInputElement>(null)
  if (ctx.window !== 'map') return null
  const { notify } = useStore.getState()
  const save = async () => {
    const { game, user } = useStore.getState()
    if (!game) return
    try {
      if (await backend.saveTextFile(`${game.id}-pins.json`, exportPins(game.id, user.customPins))) notify(`Saved ${pinCount(user.customPins.length)}`)
    } catch (e) {
      notify(`Could not save the pins: ${String(e)}`, 'error')
    }
  }
  const load = async (file: File) => {
    const { game, user, upsertCustomPin } = useStore.getState()
    if (!game) return
    try {
      const added = newPinsFrom(importPins(await file.text(), game.id), user.customPins, () => `custom-${crypto.randomUUID()}`)
      added.forEach(upsertCustomPin)
      notify(added.length ? `Added ${pinCount(added.length)}` : 'You already have those pins')
    } catch (e) {
      notify(e instanceof Error ? e.message : String(e), 'error')
    }
  }
  return (
    <Row label={label} hint={hint}>
      <div className="flex gap-1.5">
        <Button onClick={() => void save()}>Export</Button>
        <Button onClick={() => fileRef.current?.click()}>Import</Button>
      </div>
      <input
        ref={fileRef}
        type="file"
        accept="application/json,.json"
        className="hidden"
        onChange={(e) => {
          const f = e.target.files?.[0]
          if (f) void load(f)
          e.target.value = ''
        }}
      />
    </Row>
  )
}

/** Forgets the path walked so far. */
function ClearTrail({ label, hint }: ControlProps) {
  return (
    <Row label={label} hint={hint}>
      <Button onClick={clearTrail}>Clear</Button>
    </Row>
  )
}

function BackKey({ label, hint, ctx }: ControlProps) {
  const current = useSettings((s) => s.app?.back_key ?? null)
  const save = useSaveApp(ctx)
  const set = (key: Hotkey | null) => void save(() => backend.setBackKey(key))
  return (
    <Row label={label} hint={hint}>
      <HotkeyInput value={current} action="teleport_back" label="Teleport back key" onChange={set} onClear={() => set(null)} />
    </Row>
  )
}

/** Two keys per zoom direction; each can be changed or turned off. */
function zoomKey(action: HotkeyAction & keyof ZoomKeys): ComponentType<ControlProps> {
  return function ZoomKey({ label, hint, ctx }) {
    const keys = useSettings((s) => s.app?.zoom_keys ?? null)
    const save = useSaveApp(ctx)
    if (!keys) return null
    const setSlot = (slot: number, hotkey: Hotkey | null) => {
      const slots: ZoomKeys[typeof action] = [...keys[action]]
      slots[slot] = hotkey
      void save(() => backend.setZoomKeys({ ...keys, [action]: slots }))
    }
    return (
      <Row label={label} hint={hint}>
        {keys[action].map((hotkey, slot) => (
          <HotkeyInput
            key={slot}
            action={action}
            value={hotkey}
            label={`${typeof label === 'string' ? label : action} key ${slot + 1}`}
            onChange={(h) => setSlot(slot, h)}
            onClear={() => setSlot(slot, null)}
          />
        ))}
      </Row>
    )
  }
}

function ResetZoomKeys({ label, hint, ctx }: ControlProps) {
  const save = useSaveApp(ctx)
  return (
    <Row label={label} hint={hint}>
      <Button onClick={() => void save(() => backend.setZoomKeys(null))}>Reset</Button>
    </Row>
  )
}

const CORNERS: { id: Corner; label: string }[] = [
  { id: 'top_left', label: 'Top left' },
  { id: 'top_right', label: 'Top right' },
  { id: 'bottom_left', label: 'Bottom left' },
  { id: 'bottom_right', label: 'Bottom right' },
]

function monitorLabel(m: MonitorInfo, i: number): string {
  return `Monitor ${i + 1} (${m.width}×${m.height}${m.primary ? ', main' : ''})`
}

function MapWindow({ label, hint, ctx }: ControlProps) {
  const current = useSettings((s) => s.app?.map_window ?? null)
  const save = useSaveApp(ctx)
  const [monitors, setMonitors] = useState<MonitorInfo[]>([])
  useEffect(() => {
    void backend.listMonitors().then(setMonitors)
  }, [])
  // A second screen if there is one: the map next to the game, not over it.
  const preferred = monitors.find((m) => !m.primary) ?? monitors[0]
  const set = (monitor: string | null) => void save(() => backend.setMapWindow(monitor))
  return (
    <>
      <Toggle label={label} hint={hint} checked={current !== null} disabled={!preferred} onChange={(on) => set(on ? (preferred?.name ?? null) : null)} />
      {current !== null && monitors.length > 1 && (
        <Row label="Monitor">
          <select
            value={current}
            onChange={(e) => set(e.target.value)}
            aria-label="Monitor for the map window"
            className="rounded-md bg-black/30 px-1.5 py-1 text-[12px] text-ink outline-none"
          >
            {monitors.map((m, i) => (
              <option key={m.name} value={m.name} className="bg-panel-solid">
                {monitorLabel(m, i)}
              </option>
            ))}
            {!monitors.some((m) => m.name === current) && <option value={current}>Disconnected monitor</option>}
          </select>
        </Row>
      )}
    </>
  )
}

function FogOfWar({ label, hint, ctx }: ControlProps) {
  const enabled = useSettings((s) => s.ui.fogOfWar)
  const hideMap = useSettings((s) => s.ui.fogHideMap)
  const hidePins = useSettings((s) => s.ui.fogHidePins)
  const radius = useSettings((s) => s.ui.fogRadius)
  const units = useSettings((s) => s.ui.units)
  const setUi = useSettings((s) => s.setUi)
  const [confirming, setConfirming] = useState(false)
  return (
    <>
      <Toggle label={label} hint={hint} checked={enabled} onChange={(fogOfWar) => setUi({ fogOfWar })} />
      <div className="ml-3 border-l border-line pl-3">
        <Toggle
          label="Hide unexplored map"
          hint="Cover what you haven't explored instead of shading it"
          checked={hideMap}
          disabled={!enabled}
          onChange={(fogHideMap) => setUi({ fogHideMap })}
        />
        <Toggle
          label="Hide pins in unexplored areas"
          hint="On the map, in search and in progress lists; your own, found and tracked pins always show"
          checked={hidePins}
          disabled={!enabled}
          onChange={(fogHidePins) => setUi({ fogHidePins })}
        />
        {enabled && (
          <div className="py-1">
            <Slider
              label="Reveal distance"
              value={radius}
              min={25}
              max={300}
              step={25}
              format={(m) => formatDistance(m, units)}
              onChange={(fogRadius) => setUi({ fogRadius })}
            />
            <span className="block text-[11px] text-ink-faint">How far around you the map is revealed as you explore</span>
          </div>
        )}
      </div>
      {ctx.window === 'map' && (
        <div className="flex items-center justify-end gap-2 pb-1">
          {confirming && <span className="text-[11px] text-ink-faint">Forget explored areas in this world?</span>}
          <Button
            tone={confirming ? 'danger' : 'default'}
            onClick={() => {
              if (!confirming) return setConfirming(true)
              setConfirming(false)
              void resetFog()
            }}
          >
            {confirming ? 'Reset' : 'Reset explored areas'}
          </Button>
          {confirming && <Button onClick={() => setConfirming(false)}>Cancel</Button>}
        </div>
      )}
    </>
  )
}

function MinimapCorner({ label, hint, ctx }: ControlProps) {
  const gameId = ctx.game?.id
  const hud = useGameHud(gameId)
  const { anchor, width, height } = hud
  const at = (corner: Corner) => {
    const a = cornerAnchor(corner)
    return a.x === anchor.x && a.y === anchor.y
  }
  return (
    <div className="py-1">
      <span className="block text-[13px]">{label}</span>
      {hint && <span className="block text-[11px] text-ink-faint">{hint}</span>}
      <div className="grid grid-cols-2 gap-1.5 pt-1.5">
        {CORNERS.map((c) => (
          <Button key={c.id} tone={at(c.id) ? 'primary' : 'default'} onClick={() => setMinimapLayout(gameId, { anchor: cornerAnchor(c.id), width, height })}>
            {c.label}
          </Button>
        ))}
      </div>
    </div>
  )
}

function MinimapOpacity({ label }: ControlProps) {
  const value = useSettings((s) => s.ui.hud.opacity)
  const setHud = useSettings((s) => s.setHud)
  return <Slider label={label} value={value} min={0.3} max={1} step={0.02} format={pct} onChange={(opacity) => setHud({ opacity })} />
}

/** Who you're connected with, or why nobody is (the map window runs the connection). */
function TogetherStatus({ ctx }: { ctx: SettingsContext }) {
  const session = useSyncStore((s) => s.session)
  const peers = readyPeers({ peers: useSyncStore((s) => s.peers) })
  if (ctx.window !== 'map') return null
  const text = !session
    ? 'Waiting for a multiplayer game'
    : peers.length === 0
      ? 'Nobody else on this server is using Game Maps yet'
      : `Connected with ${peers.map((p) => p.name ?? 'a player').join(', ')}`
  return <p className="px-1 text-[12px] text-ink-faint">{text}</p>
}

const PlayTogetherToggle = gameToggle('playTogether')

function PlayTogether(props: ControlProps) {
  const on = useSettings((s) => gameSettings(s.ui, props.ctx.game?.id).playTogether)
  return (
    <>
      <PlayTogetherToggle {...props} />
      {on && <TogetherStatus ctx={props.ctx} />}
    </>
  )
}

function useLinkStatus() {
  const [status, setStatus] = useState<LinkStatus | null>(null)
  useEffect(() => {
    const unlisten = events.status(setStatus)
    void backend.status().then(setStatus)
    return () => void unlisten.then((f) => f())
  }, [])
  return status
}

function Diagnostics({ label }: ControlProps) {
  const status = useLinkStatus()
  const [logs, setLogs] = useState<AgentLog[]>(agentLogs)
  useEffect(() => {
    const id = setInterval(() => setLogs(agentLogs()), 1000)
    return () => clearInterval(id)
  }, [])
  const capabilities = status?.capabilities
    ? Object.entries(status.capabilities)
        .filter(([, v]) => v)
        .map(([k]) => k.replace('_', ' '))
        .join(', ')
    : ''
  return (
    <div className="space-y-1.5 py-1">
      <span className="block text-[13px]">{label}</span>
      <p className="text-[12px] text-ink-dim">
        {status?.state ?? 'starting'}
        {status?.pid ? ` · pid ${status.pid}` : ''}
        {capabilities && ` · ${capabilities}`}
      </p>
      <pre className="scroll-thin max-h-40 overflow-auto whitespace-pre-wrap rounded-lg bg-black/40 p-2 font-mono text-[10.5px] leading-relaxed text-ink-dim">
        {logs.length ? logs.map((l) => `[${l.level}] ${l.message}`).join('\n') : 'No agent messages yet.'}
      </pre>
    </div>
  )
}

function LogsFolder({ label, hint, ctx }: ControlProps) {
  return (
    <Row label={label} hint={hint}>
      <Button onClick={() => void backend.openLogsFolder().catch((e: unknown) => ctx.onError(String(e)))}>
        <FolderOpen className="size-3.5" /> Open
      </Button>
    </Row>
  )
}

function ProblemReport({ label, hint, ctx }: ControlProps) {
  const [saved, setSaved] = useState(false)
  const save = () => {
    setSaved(false)
    saveProblemReport()
      .then(setSaved)
      .catch((e: unknown) => ctx.onError(String(e)))
  }
  return (
    <div className="py-1">
      <span className="block text-[13px]">{label}</span>
      {hint && <span className="block pb-1.5 text-[11px] text-ink-faint">{hint}</span>}
      <div className="flex gap-1.5">
        <Button onClick={save}>
          <FileText className="size-3.5" /> Save report
        </Button>
        <Button onClick={() => void backend.openIssuePage().catch((e: unknown) => ctx.onError(String(e)))}>
          <ExternalLink className="size-3.5" /> Open issues page
        </Button>
      </div>
      {saved && <p className="pt-1.5 text-[12px] text-ink-dim">Saved. Attach the file to a new issue, or send it to whoever is helping you.</p>}
    </div>
  )
}

/** Dev tooling: saves every loaded item and point of interest for `tools/pins`. */
function ExportWorld({ label, hint }: ControlProps) {
  const connected = useLinkStatus()?.state === 'connected'
  const [state, setState] = useState<{ busy: boolean; result?: string }>({ busy: false })
  const run = async () => {
    setState({ busy: true })
    try {
      setState({ busy: false, result: `Saved to ${await backend.exportWorld()}` })
    } catch (e) {
      setState({ busy: false, result: `Export failed: ${String(e)}` })
    }
  }
  return (
    <div className="py-1">
      <Row label={label} hint={hint}>
        <Button disabled={!connected || state.busy} onClick={() => void run()}>
          {state.busy ? 'Exporting…' : 'Export'}
        </Button>
      </Row>
      {state.result && <p className="break-all text-[11px] text-ink-dim">{state.result}</p>}
    </div>
  )
}

// Entries ----------------------------------------------------------------------------

export const SETTINGS: SettingDef[] = [
  // General
  { id: 'units', category: 'general', scope: 'all', label: 'Metric units', hint: 'Meters and kilometers instead of feet and miles', keywords: 'distance imperial feet', Control: Units },
  { id: 'ui-scale', category: 'general', scope: 'all', label: 'Auto interface size', hint: 'Scales panels and text to your screen', keywords: 'ui scale zoom text font bigger smaller', Control: InterfaceSize },
  { id: 'connect-all', category: 'general', scope: 'all', label: 'Connect to games automatically', hint: "For every game: connects when it starts and downloads its map the first time. When off, nothing happens until you click Play or Connect", keywords: 'hook attach auto manual inject download map all games', Control: ConnectAll },
  { id: 'map-data', category: 'general', scope: 'game', label: 'Map data', keywords: 'download update pack tiles storage', Control: MapData },
  { id: 'connect', category: 'general', scope: 'game', label: 'Connect to the game', hint: 'Automatically when the game starts, or only when you click Connect. Automatic also downloads the map the first time', keywords: 'hook attach auto manual inject', applies: live, Control: ConnectMode },
  { id: 'autostart', category: 'general', scope: 'all', label: 'Start with Windows', hint: 'Runs quietly in the tray, so the map is ready whenever you play', keywords: 'startup boot login tray', Control: Autostart },
  { id: 'auto-update', category: 'general', scope: 'all', label: 'Install updates automatically', hint: 'Updates install while no game is running', keywords: 'version upgrade check', Control: AutoUpdate },
  { id: 'analytics', category: 'general', scope: 'all', label: 'Share anonymous usage counts', hint: 'Which games you play, how long, which features you use and what went wrong, with a random ID. Never your name, location, pins or worlds. The README lists everything', keywords: 'analytics telemetry privacy tracking data statistics', Control: Analytics },

  // Keys
  { id: 'map-key', category: 'keys', scope: 'game', label: 'Open / close map', hint: "While Game Maps runs, the game doesn't see this key", keywords: 'hotkey keybind toggle', Control: MapKey },
  { id: 'back-key', category: 'keys', scope: 'all', label: 'Teleport back', hint: 'Back to where you were before your last teleport; press again to go further back', keywords: 'hotkey keybind undo return', Control: BackKey },
  { id: 'quick-mark-key', category: 'keys', scope: 'all', label: 'Quick mark', hint: 'Drops a pin where you stand, without opening the map', keywords: 'hotkey keybind pin marker', Control: actionKey('quick_mark') },
  { id: 'nearest-key', category: 'keys', scope: 'all', label: 'Track nearest', hint: "Tracks the closest thing you haven't found, in the categories shown on the map", keywords: 'hotkey keybind unfound collectible closest', Control: actionKey('nearest_unfound') },
  { id: 'accept-key', category: 'keys', scope: 'all', label: 'Accept a track request', hint: "Accepts a friend's request without opening the map", keywords: 'hotkey keybind share friend multiplayer', Control: actionKey('accept_request') },
  { id: 'zoom-in', category: 'keys', scope: 'all', label: 'Zoom in', hint: 'The open map, or the mini-map while playing', keywords: 'hotkey keybind', Control: zoomKey('zoom_in') },
  { id: 'zoom-out', category: 'keys', scope: 'all', label: 'Zoom out', hint: 'The mouse wheel also zooms the open map', keywords: 'hotkey keybind', Control: zoomKey('zoom_out') },
  { id: 'zoom-reset', category: 'keys', scope: 'all', label: 'Default zoom keys', hint: 'Numpad + and −, and = and −', keywords: 'reset restore hotkey', Control: ResetZoomKeys },

  // Full map
  { id: 'map-window', category: 'map', scope: 'all', label: 'Map window on another monitor', hint: 'Keeps the full map open on a second screen while you play; closing the window turns it off', keywords: 'second monitor screen display dual window always', Control: MapWindow },
  { id: 'map-opacity', category: 'map', scope: 'all', label: 'Opacity', hint: 'The game shows through below 100%', keywords: 'transparency see through', Control: uiSlider('opacity', { min: 0.4, max: 1, step: 0.02 }) },
  { id: 'map-layout', category: 'map', scope: 'all', label: 'Position and size', hint: 'Drag the map and mini-map where you want them, and resize them by their edges', keywords: 'move resize scale panel screen drag layout edit corner', Control: EditLayout },
  { id: 'pin-size', category: 'map', scope: 'all', label: 'Pin size', keywords: 'icon marker bigger smaller', Control: uiSlider('pinSize', { min: 0.6, max: 1.6, step: 0.05 }) },
  { id: 'teleport', category: 'map', scope: 'all', label: 'Teleport', hint: 'Shows teleport buttons on the map, in games that support it', keywords: 'jump travel fast cheat', Control: uiToggle('teleport') },
  { id: 'fog-of-war', category: 'map', scope: 'all', label: 'Fog of war', hint: "Only what you've explored is revealed, so the map doesn't spoil anything (each world keeps its own)", keywords: 'explored unexplored discover reveal radius distance range reset spoiler spoil hide pins icons', Control: FogOfWar },
  { id: 'trail', category: 'map', scope: 'all', label: 'Path history', hint: "Draws where you've walked since the app started", keywords: 'trail breadcrumbs route walked history', Control: uiToggle('showTrail') },
  { id: 'trail-clear', category: 'map', scope: 'all', label: 'Clear path history', keywords: 'trail breadcrumbs reset', Control: ClearTrail },
  { id: 'pin-files', category: 'map', scope: 'all', label: 'Pins in a file', hint: "Save this world's pins, or add pins someone sent you", keywords: 'import export share backup json', Control: PinFiles },
  { id: 'declutter', category: 'map', scope: 'all', label: 'Declutter pins', hint: 'Where pins overlap, the most important stay; the rest show as dots until you zoom in', keywords: 'overlap clutter icons', Control: uiToggle('declutter') },

  // Mini-map
  { id: 'minimap', category: 'minimap', scope: 'all', label: 'Mini-map while playing', hint: 'Click-through map in a corner of the screen', keywords: 'minimap hud radar overlay', Control: hudToggle('minimap') },
  { id: 'minimap-rotate', category: 'minimap', scope: 'all', label: 'Rotate with view', hint: 'The direction you face points up', keywords: 'minimap heading north compass', Control: hudToggle('rotateWithPlayer') },
  { id: 'minimap-auto-hide', category: 'minimap', scope: 'all', label: 'Hide for alerts', hint: 'Fades out while danger markers or pings are on or next to it, so nothing is hidden behind it', keywords: 'minimap auto hide fight enemies overlap danger', Control: hudToggle('autoHide') },
  { id: 'minimap-clock', category: 'minimap', scope: 'all', label: 'Time and weather', hint: 'The day, time, season and weather on the mini-map, in games that report them', keywords: 'clock day season', Control: hudToggle('clock') },
  { id: 'minimap-layout', category: 'minimap', scope: 'game', label: 'Position and size', hint: 'Drag it anywhere on the screen, and resize it by its edges. Each game remembers its own', keywords: 'minimap move resize width height drag layout edit', Control: EditLayout },
  { id: 'minimap-corner', category: 'minimap', scope: 'game', label: 'Corner', keywords: 'minimap position place screen', Control: MinimapCorner },
  { id: 'minimap-opacity', category: 'minimap', scope: 'all', label: 'Opacity', keywords: 'minimap transparency see through', Control: MinimapOpacity },

  // Map content
  { id: 'death-marker', category: 'content', scope: 'game', label: 'Mark where you die', hint: 'Keeps a pin where you last died, so you can get your gear back', keywords: 'death gear corpse grave', applies: live, Control: gameToggle('deathMarker') },
  { id: 'auto-found', category: 'content', scope: 'game', label: 'Mark collectibles found', hint: 'Items, weapons and notes are marked found when you walk up to them', keywords: 'auto collected pickup', applies: live, Control: gameToggle('autoFound') },
  { id: 'creatures', category: 'content', scope: 'game', label: 'Nearby creatures', hint: 'Enemies, companions and animals around you, as far as the game keeps them', keywords: 'enemies cannibals mutants npc radar', applies: live, Control: gameToggle('showCreatures') },
  { id: 'animals', category: 'content', scope: 'game', label: 'Include animals', hint: 'Wildlife as well as enemies and companions', keywords: 'creatures wildlife deer', applies: live, Control: gameToggle('showAnimals', 'showCreatures') },

  { id: 'danger-alert', category: 'content', scope: 'game', label: 'Danger alert', hint: 'A count at the top of the screen, and red arrows at the screen edges pointing toward hostiles close by', keywords: 'enemies warning threat nearby cannibals hud', applies: live, Control: gameToggle('dangerAlert') },
  { id: 'danger-distance', category: 'content', scope: 'game', label: 'Alert distance', keywords: 'danger radius range enemies', applies: live, Control: DangerDistance },

  // Tracking
  { id: 'route', category: 'tracking', scope: 'all', label: 'Route to what you track', hint: 'The walking path on the map and mini-map', keywords: 'path navigation directions', Control: uiToggle('showRoute') },
  { id: 'waypoint', category: 'tracking', scope: 'all', label: 'Waypoint in the game', hint: 'Shows what you track over the game, with its distance', keywords: 'marker objective hud compass', Control: hudToggle('waypoint') },

  // Multiplayer
  { id: 'play-together', category: 'multiplayer', scope: 'game', label: 'Play together', hint: 'Connect with others running Game Maps on your server to share what you track, map pings and pins', keywords: 'sync friends coop share', applies: live, Control: PlayTogether },

  { id: 'connection-check', category: 'multiplayer', scope: 'game', label: 'Connection check', hint: "Tests your connection to friends, and what's in the way if you can't reach them", keywords: 'network firewall nat vpn troubleshoot friends cant connect', applies: (ctx) => live(ctx) && ctx.window === 'map', Control: ({ label, hint, ctx }) => <ConnectionCheck label={label} hint={hint} gameId={ctx.game?.id} /> },
  { id: 'share-progress', category: 'multiplayer', scope: 'game', label: 'Share progress', hint: 'Found items and explored areas are shared with friends who also turned this on', keywords: 'found fog explored merge friends', applies: live, Control: gameToggle('shareProgress', 'playTogether') },
  { id: 'friend-markers', category: 'multiplayer', scope: 'game', label: 'Friends over the game', hint: 'Shows where other players are, with their name and distance', keywords: 'players teammates marker hud', applies: live, Control: gameToggle('friendMarkers') },
  { id: 'pings-in-game', category: 'multiplayer', scope: 'game', label: 'Pings in the game', hint: 'Map pings from you and your friends show over the game for 30 seconds', keywords: 'ping marker friends hud', applies: live, Control: gameToggle('pingsInGame') },

  // Advanced
  { id: 'diagnostics', category: 'advanced', scope: 'all', label: 'Connection', keywords: 'diagnostics logs debug agent status', Control: Diagnostics },
  { id: 'star', category: 'advanced', scope: 'all', label: 'Enjoying Game Maps?', hint: 'A star on GitHub helps other players find it', keywords: 'github star project contribute source code', Control: StarProject },
  { id: 'problem-report', category: 'advanced', scope: 'all', label: 'Report a problem', hint: 'Saves your logs and setup to one file you can read first, then attach. Nothing is sent by itself', keywords: 'bug issue crash diagnostics logs help support', Control: ProblemReport },
  { id: 'logs', category: 'advanced', scope: 'all', label: 'Log files', hint: 'For bug reports', keywords: 'diagnostics debug folder', Control: LogsFolder },
  { id: 'export-world', category: 'advanced', scope: 'all', label: 'Export world data', hint: 'Saves every loaded item and point of interest (for map makers)', keywords: 'dump developer pins', Control: ExportWorld },
]

/** The settings that apply in `ctx`, in panel order. */
export function applicableSettings(ctx: SettingsContext): SettingDef[] {
  return SETTINGS.filter((s) => (s.scope === 'all' || ctx.game) && (!s.applies || s.applies(ctx)))
}

export function categoryOf(id: CategoryId): Category {
  return CATEGORIES.find((c) => c.id === id)!
}

/** Icon for the per-game marker and game picker. */
export const GameIcon = Gamepad2
