// Every setting, where it's filed and how it's changed. The settings panel lists these
// by category and searches them, so a new setting only needs an entry here.
//
// `scope: 'game'` settings are kept per game (shown for the game picked in the panel);
// the rest apply to every game.
//
// The registry is data with its controls alongside; edits reload the page in development.
// oxlint-disable react/only-export-components

import { Download, FolderOpen, Gamepad2, Keyboard, Map as MapIcon, MapPinned, Navigation, PictureInPicture2, RefreshCw, SlidersHorizontal, Users, Wrench, type LucideIcon } from 'lucide-react'
import { useCallback, useEffect, useState, type ComponentType, type ReactNode } from 'react'
import { agentLogs } from '../lib/agentLogs'
import { backend, events, type AppSettings, type GameEntry, type HookMode, type UpdateState, type ZoomKeys } from '../lib/backend'
import { maxMinimapSize, MIN_MINIMAP_SIZE } from '../lib/minimap'
import type { AgentLog, Corner, Hotkey, HotkeyAction, LinkStatus } from '../lib/protocol'
import { readyPeers, useSyncStore } from '../lib/sync/sync'
import { MAX_UI_SCALE, MIN_UI_SCALE, useUiScale } from '../lib/uiScale'
import { Button, Segmented, Slider, Toggle } from '../ui/controls'
import { HotkeyInput } from '../ui/HotkeyInput'
import { gameSettings, type GameSettings, type HudSettings, type Settings } from './model'
import { mapHotkey, useSettings } from './store'

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
const px = (v: number) => `${v}px`
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

/** A switch for a setting of the panel's game. */
function gameToggle(key: keyof GameSettings, requires?: keyof GameSettings): ComponentType<ControlProps> {
  return function GameToggle({ label, hint, ctx }) {
    const id = ctx.game?.id ?? null
    const checked = useSettings((s) => gameSettings(s.ui, id)[key])
    const enabled = useSettings((s) => !requires || gameSettings(s.ui, id)[requires])
    const setGameSettings = useSettings((s) => s.setGameSettings)
    if (!id) return null
    return <Toggle label={label} hint={hint} checked={checked} disabled={!enabled} onChange={(v) => setGameSettings(id, { [key]: v })} />
  }
}

function uiSlider(
  key: 'opacity' | 'panelScale' | 'pinSize',
  range: { min: number; max: number; step: number },
): ComponentType<ControlProps> {
  return function UiSlider({ label }) {
    const value = useSettings((s) => s.ui[key])
    const setUi = useSettings((s) => s.setUi)
    return <Slider label={label} value={value} {...range} format={pct} onChange={(v) => setUi({ [key]: v })} />
  }
}

function hudSizeSlider(key: 'width' | 'height'): ComponentType<ControlProps> {
  return function HudSizeSlider({ label }) {
    const value = useSettings((s) => s.ui.hud[key])
    const setHud = useSettings((s) => s.setHud)
    const max = maxMinimapSize(window.screen)[key]
    return <Slider label={label} value={Math.min(value, max)} min={MIN_MINIMAP_SIZE} max={max} step={10} format={px} onChange={(v) => setHud({ [key]: v })} />
  }
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

function ConnectMode({ label, hint, ctx }: ControlProps) {
  const id = ctx.game?.id
  const mode = useSettings((s) => (id && s.app?.hook_modes[id]) || 'auto')
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

function MinimapCorner({ label, hint }: ControlProps) {
  const corner = useSettings((s) => s.ui.hud.corner)
  const setHud = useSettings((s) => s.setHud)
  return (
    <div className="py-1">
      <span className="block text-[13px]">{label}</span>
      {hint && <span className="block text-[11px] text-ink-faint">{hint}</span>}
      <div className="grid grid-cols-2 gap-1.5 pt-1.5">
        {CORNERS.map((c) => (
          <Button key={c.id} tone={corner === c.id ? 'primary' : 'default'} onClick={() => setHud({ corner: c.id })}>
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
  { id: 'connect', category: 'general', scope: 'game', label: 'Connect to the game', hint: 'Automatically when the game starts, or only when you click Connect', keywords: 'hook attach auto manual inject', applies: live, Control: ConnectMode },
  { id: 'autostart', category: 'general', scope: 'all', label: 'Start with Windows', hint: 'Runs quietly in the tray, so the map is ready whenever you play', keywords: 'startup boot login tray', Control: Autostart },
  { id: 'auto-update', category: 'general', scope: 'all', label: 'Install updates automatically', hint: 'Updates install while no game is running', keywords: 'version upgrade check', Control: AutoUpdate },

  // Keys
  { id: 'map-key', category: 'keys', scope: 'game', label: 'Open / close map', hint: "While Game Maps runs, the game doesn't see this key", keywords: 'hotkey keybind toggle', Control: MapKey },
  { id: 'back-key', category: 'keys', scope: 'all', label: 'Teleport back', hint: 'Back to where you were before your last teleport; press again to go further back', keywords: 'hotkey keybind undo return', Control: BackKey },
  { id: 'zoom-in', category: 'keys', scope: 'all', label: 'Zoom in', hint: 'The open map, or the mini-map while playing', keywords: 'hotkey keybind', Control: zoomKey('zoom_in') },
  { id: 'zoom-out', category: 'keys', scope: 'all', label: 'Zoom out', hint: 'The mouse wheel also zooms the open map', keywords: 'hotkey keybind', Control: zoomKey('zoom_out') },
  { id: 'zoom-reset', category: 'keys', scope: 'all', label: 'Default zoom keys', hint: 'Numpad + and −, and = and −', keywords: 'reset restore hotkey', Control: ResetZoomKeys },

  // Full map
  { id: 'map-opacity', category: 'map', scope: 'all', label: 'Opacity', hint: 'The game shows through below 100%', keywords: 'transparency see through', Control: uiSlider('opacity', { min: 0.4, max: 1, step: 0.02 }) },
  { id: 'map-size', category: 'map', scope: 'all', label: 'Size', keywords: 'scale panel screen', Control: uiSlider('panelScale', { min: 0.5, max: 1, step: 0.05 }) },
  { id: 'pin-size', category: 'map', scope: 'all', label: 'Pin size', keywords: 'icon marker bigger smaller', Control: uiSlider('pinSize', { min: 0.6, max: 1.6, step: 0.05 }) },
  { id: 'declutter', category: 'map', scope: 'all', label: 'Declutter pins', hint: 'Where pins overlap, the most important stay; the rest show as dots until you zoom in', keywords: 'overlap clutter icons', Control: uiToggle('declutter') },

  // Mini-map
  { id: 'minimap', category: 'minimap', scope: 'all', label: 'Mini-map while playing', hint: 'Click-through map in a corner of the screen', keywords: 'minimap hud radar overlay', Control: hudToggle('minimap') },
  { id: 'minimap-rotate', category: 'minimap', scope: 'all', label: 'Rotate with view', hint: 'The direction you face points up', keywords: 'minimap heading north compass', Control: hudToggle('rotateWithPlayer') },
  { id: 'minimap-corner', category: 'minimap', scope: 'all', label: 'Corner', keywords: 'minimap position place screen', Control: MinimapCorner },
  { id: 'minimap-width', category: 'minimap', scope: 'all', label: 'Width', keywords: 'minimap size', Control: hudSizeSlider('width') },
  { id: 'minimap-height', category: 'minimap', scope: 'all', label: 'Height', keywords: 'minimap size', Control: hudSizeSlider('height') },
  { id: 'minimap-opacity', category: 'minimap', scope: 'all', label: 'Opacity', keywords: 'minimap transparency see through', Control: MinimapOpacity },

  // Map content
  { id: 'auto-found', category: 'content', scope: 'game', label: 'Mark collectibles found', hint: 'Items, weapons and notes are marked found when you walk up to them', keywords: 'auto collected pickup', applies: live, Control: gameToggle('autoFound') },
  { id: 'creatures', category: 'content', scope: 'game', label: 'Nearby creatures', hint: 'Enemies, companions and animals around you, as far as the game keeps them', keywords: 'enemies cannibals mutants npc radar', applies: live, Control: gameToggle('showCreatures') },
  { id: 'animals', category: 'content', scope: 'game', label: 'Include animals', hint: 'Wildlife as well as enemies and companions', keywords: 'creatures wildlife deer', applies: live, Control: gameToggle('showAnimals', 'showCreatures') },

  // Tracking
  { id: 'route', category: 'tracking', scope: 'all', label: 'Route to what you track', hint: 'The walking path on the map and mini-map', keywords: 'path navigation directions', Control: uiToggle('showRoute') },
  { id: 'waypoint', category: 'tracking', scope: 'all', label: 'Waypoint in the game', hint: 'Shows what you track over the game, with its distance', keywords: 'marker objective hud compass', Control: hudToggle('waypoint') },

  // Multiplayer
  { id: 'play-together', category: 'multiplayer', scope: 'game', label: 'Play together', hint: 'Connect with others running Game Maps on your server to share what you track, map pings and pins', keywords: 'sync friends coop share', applies: live, Control: PlayTogether },

  // Advanced
  { id: 'diagnostics', category: 'advanced', scope: 'all', label: 'Connection', keywords: 'diagnostics logs debug agent status', Control: Diagnostics },
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
