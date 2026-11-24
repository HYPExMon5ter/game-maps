import { Check, Copy, Footprints, Info, MapPinPlus, Navigation, Pencil, Radio, Trash2, Zap } from 'lucide-react'
import { Activity, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type MouseEvent as ReactMouseEvent, type PointerEvent as ReactPointerEvent, type RefObject } from 'react'
import { findPin, navigateTo, newCustomPinAt, teleportToMapPoint, teleportToWorld } from '../lib/actions'
import { backend } from '../lib/backend'
import type { GameMap, Pin } from '../lib/game'
import type { CompanionStatus, Vec3 } from '../lib/protocol'
import { fitRect, HANDLES, minMapPanel, moveRect, resizeRect, toFractions, toPixels, type Handle, type Rect, type Size } from '../lib/layout'
import { foundSet } from '../lib/lookup'
import { toastMs } from '../lib/toast'
import { toLngLat, toWorld } from '../lib/projection'
import { readyPeers, sendPing, useSyncStore } from '../lib/sync/sync'
import { useUiScale, useWindowSize } from '../lib/uiScale'
import { MapView, type MapContextMenuEvent } from '../map/MapView'
import { Cave3DView } from '../map/Cave3DView'
import { use3DMode, useMap3D } from '../state/view3d'
import { Cave3DControls } from './Cave3DControls'
import { Surface3DControls } from './Surface3DControls'
import { useCanTeleport, useTeleportOffered } from '../lib/teleport'
import { selectViewLayer, useStore } from '../state/store'
import { MissingMap } from './MissingMap'
import { Download3D } from './Download3D'
import { FogTip } from './Onboarding'
import { Compass } from './Compass'
import { LayoutEditor } from './LayoutEditor'
import { LiveDetails } from './LiveDetails'
import { PinDetails } from './PinDetails'
import { PinEditor } from './PinEditor'
import { SettingsPanel } from './SettingsPanel'
import { Sidebar } from './Sidebar'
import { SyncRequests } from './SyncRequests'
import { applyRect, trackDrag } from './dragRect'
import { useDismiss } from './useDismiss'
import { LinkProblem, TopBar } from './TopBar'
import { useSettings } from '../settings/store'
import { IS_MAP_WINDOW } from '../lib/view'

/** Tells a companion to walk to a spot. */
function sendCompanion(c: CompanionStatus, at: Vec3, map: GameMap) {
  const p = map.projection
  const { notify } = useStore.getState()
  backend.companionOrder(c.id, `go:${at[p.east]},${at[p.north]}`).then(
    () => notify(`${c.name} is on the way`),
    (e) => notify(`${c.name} can't go there: ${String(e)}`, 'error'),
  )
}

function ContextMenu({
  at,
  scale,
  onClose,
  onAddPin,
}: {
  at: MapContextMenuEvent
  /** Interface scale of the layer the menu is drawn in; `at` is in map pixels. */
  scale: number
  onClose: () => void
  onAddPin: (pin: Pin) => void
}) {
  const teleportOffered = useTeleportOffered()
  const canTeleport = useCanTeleport()
  const map = useStore((s) => s.map)
  const notify = useStore((s) => s.notify)
  const layer = useStore(selectViewLayer)
  const together = useSyncStore((s) => readyPeers(s).length > 0)
  const gamePings = useStore((s) => !!s.status?.capabilities?.game_pings)
  const canSend = useStore((s) => !!s.game?.sendCompanions)
  const companions = useStore((s) => s.snapshot?.companions)
  const sendable = useMemo(() => (canSend ? (companions ?? []).filter((c) => c.alive) : []), [canSend, companions])
  const world = at.world ?? (map ? toWorld(map.projection, at.lngLat) : null)
  const item = 'flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-[13px] hover:bg-panel-raised disabled:opacity-40'
  const pin = at.pinId ? findPin(at.pinId) : undefined
  // Back inside the map when it wouldn't fit below or to the right of the click: the
  // map's panel cuts off what sticks out.
  const ref = useRef<HTMLDivElement>(null)
  const [shift, setShift] = useState({ x: 0, y: 0 })
  useLayoutEffect(() => {
    const el = ref.current
    const parent = el?.offsetParent
    if (!el || !parent) return
    const menu = el.getBoundingClientRect()
    const area = parent.getBoundingClientRect()
    const x = Math.max(0, menu.width + at.x + 8 - area.width)
    const y = Math.max(0, menu.height + at.y + 8 - area.height)
    setShift((s) => (s.x === x && s.y === y ? s : { x, y }))
  }, [at, pin])
  useDismiss(true, ref, onClose)
  return (
    <div
      ref={ref}
      className="glass pointer-events-auto absolute z-20 w-[200px] rounded-xl p-1"
      style={{ left: Math.max(0, at.x - shift.x) / scale, top: Math.max(0, at.y - shift.y) / scale }}
      onMouseDown={(e) => e.stopPropagation()}
      onContextMenu={(e) => e.preventDefault()}
    >
      {pin ? (
        <PinMenuItems pin={pin} item={item} onEdit={onAddPin} onClose={onClose} />
      ) : (
        <button className={item} onClick={() => (onAddPin(newCustomPinAt(at.lngLat, at.world)), onClose())}>
          <MapPinPlus className="size-4" /> Add pin here
        </button>
      )}
      {!pin && (
        <button className={item} onClick={() => (void navigateTo(at.lngLat, at.world, layer), onClose())}>
          <Navigation className="size-4" /> Navigate here
        </button>
      )}
      {teleportOffered && (
        <button className={item} disabled={!canTeleport} onClick={() => (void (at.world ? teleportToWorld(at.world, layer) : teleportToMapPoint(at.lngLat)), onClose())}>
          <Zap className="size-4" /> Teleport here
        </button>
      )}
      {world &&
        map &&
        sendable.map((c) => (
          <button key={c.id} className={item} onClick={() => (sendCompanion(c, world, map), onClose())}>
            <Footprints className="size-4" /> Send {c.name} here
          </button>
        ))}
      {(together || gamePings) && world && (
        <button className={item} onClick={() => (sendPing(world, layer), onClose())}>
          <Radio className="size-4" /> Ping here
        </button>
      )}
      {world && (
        <button
          className={item}
          onClick={() => {
            void navigator.clipboard.writeText(`${world.x.toFixed(1)}, ${world.z.toFixed(1)}`)
            notify('Coordinates copied')
            onClose()
          }}
        >
          <Copy className="size-4" /> {world.x.toFixed(0)}, {world.z.toFixed(0)}
        </button>
      )}
    </div>
  )
}

/** What can be done with a pin, on the menu for it. */
function PinMenuItems({ pin, item, onEdit, onClose }: { pin: Pin; item: string; onEdit: (pin: Pin) => void; onClose: () => void }) {
  const tracked = useStore((s) => s.tracked?.kind === 'pin' && s.tracked.id === pin.id)
  const found = useStore((s) => foundSet(s.user.found).has(pin.id))
  const { selectPin, setTracked, toggleFound, deleteCustomPin } = useStore.getState()
  const run = (action: () => void) => () => {
    action()
    onClose()
  }
  return (
    <>
      <p className="truncate px-2.5 pb-1 pt-1.5 text-[12px] font-semibold text-ink-dim">{pin.name}</p>
      <button className={item} onClick={run(() => selectPin(pin.id))}>
        <Info className="size-4" /> Details
      </button>
      <button className={item} onClick={run(() => setTracked(tracked ? null : { kind: 'pin', id: pin.id }))}>
        <Navigation className="size-4" /> {tracked ? 'Stop tracking' : 'Track'}
      </button>
      {!pin.custom && (
        <button className={item} onClick={run(() => toggleFound(pin.id))}>
          <Check className="size-4" /> {found ? 'Not found yet' : 'Mark found'}
        </button>
      )}
      {pin.custom && (
        <>
          <button className={item} onClick={run(() => onEdit(pin))}>
            <Pencil className="size-4" /> Edit
          </button>
          <button className={`${item} text-danger`} onClick={run(() => deleteCustomPin(pin.id))}>
            <Trash2 className="size-4" /> Delete
          </button>
        </>
      )}
      <div className="mx-2 my-1 border-t border-line" />
    </>
  )
}

function Toast() {
  const toast = useStore((s) => s.toast)
  const dismiss = useStore((s) => s.dismissToast)
  useEffect(() => {
    if (!toast) return
    const t = setTimeout(() => dismiss(toast.id), toastMs(toast.text, toast.tone))
    return () => clearTimeout(t)
  }, [toast, dismiss])
  if (!toast) return null
  // As wide as its text up to a limit, then it wraps: an error can run to a few lines.
  return (
    <div
      role="status"
      className={`glass pointer-events-none absolute bottom-5 left-1/2 z-40 w-max max-w-[min(90%,560px)] -translate-x-1/2 rounded-[18px] px-4 py-2 text-center text-[13px] ${
        toast.tone === 'error' ? 'text-danger' : ''
      }`}
    >
      {toast.text}
    </div>
  )
}

/** Where each edge and corner of the floating map is grabbed to resize it. */
const GRIP_STYLE: Record<Handle, string> = {
  n: 'inset-x-4 top-0 h-1.5 cursor-ns-resize',
  s: 'inset-x-4 bottom-0 h-1.5 cursor-ns-resize',
  e: 'inset-y-4 right-0 w-1.5 cursor-ew-resize',
  w: 'inset-y-4 left-0 w-1.5 cursor-ew-resize',
  nw: 'left-0 top-0 size-4 cursor-nwse-resize',
  ne: 'right-0 top-0 size-4 cursor-nesw-resize',
  se: 'bottom-0 right-0 size-4 cursor-nwse-resize',
  sw: 'bottom-0 left-0 size-4 cursor-nesw-resize',
}

/** What in the top bar does something itself, so pressing it doesn't move the map. */
const BAR_CONTROLS = 'button, a, input, select, textarea, [data-menu]'

/**
 * The floating map moves and resizes like a window: dragged by its top bar, resized by
 * its edges and corners. Saved when the drag ends (see `ui/dragRect`).
 */
function usePanelDrag(panelRef: RefObject<HTMLDivElement | null>, rect: Rect, screen: Size, min: Size) {
  const begin = (e: ReactPointerEvent, next: (dx: number, dy: number) => Rect) => {
    let current = rect
    trackDrag(
      e,
      (dx, dy) => {
        current = next(dx, dy)
        applyRect(panelRef.current, current)
      },
      () => {
        if (current !== rect) useSettings.getState().setUi({ mapPanel: toFractions(current, screen) })
      },
    )
  }
  return {
    move: (e: ReactPointerEvent) => {
      if (e.button !== 0 || (e.target as HTMLElement).closest(BAR_CONTROLS)) return
      begin(e, (dx, dy) => moveRect(rect, dx, dy, screen))
    },
    resize: (handle: Handle) => (e: ReactPointerEvent) => {
      if (e.button !== 0) return
      begin(e, (dx, dy) => resizeRect(rect, handle, dx, dy, min, screen, screen))
    },
  }
}

/** Map area covered by the top bar and sidebar (see the layout below), in screen px. */
function panelPadding(uiScale: number) {
  const gap = 12
  const topBar = 48
  const sidebar = 300
  return {
    top: (gap + topBar + gap) * uiScale + gap,
    left: (gap + sidebar + gap) * uiScale + gap,
    right: gap * 2,
    bottom: gap * 2,
  }
}

/** The full map: over the game (sized and see-through per the settings), or filling the map window. */
export function FullMap() {
  const opacity = useSettings((s) => (IS_MAP_WINDOW ? 1 : s.ui.opacity))
  const fills = useSettings((s) => IS_MAP_WINDOW || s.ui.mapMaximized)
  const mapPanel = useSettings((s) => s.ui.mapPanel)
  const screen = useWindowSize()
  const panelRef = useRef<HTMLDivElement>(null)
  const editingLayout = useStore((s) => s.editingLayout)
  const uiScale = useUiScale()
  const minPanel = useMemo(() => minMapPanel(uiScale), [uiScale])
  const rect = useMemo(() => {
    const r = fitRect(toPixels(mapPanel, screen), screen)
    return fills ? { left: 0, top: 0, ...screen } : fitRect({ ...r, width: Math.max(r.width, minPanel.width), height: Math.max(r.height, minPanel.height) }, screen)
  }, [mapPanel, screen, fills, minPanel])
  const hasSelection = useStore((s) => s.selectedPinId !== null)
  const hasLiveSelection = useStore((s) => s.selectedLiveId !== null)
  const tracking = useStore((s) => s.tracked !== null)
  const [menu, setMenu] = useState<MapContextMenuEvent | null>(null)
  const closeMenu = useCallback(() => setMenu(null), [])
  const [editing, setEditing] = useState<Pin | null>(null)
  const [settingsOpen, setSettingsOpen] = useState(false)
  const shown = useStore((s) => IS_MAP_WINDOW || s.mode === 'open')
  const fitPadding = useMemo(() => panelPadding(uiScale), [uiScale])
  const drag = usePanelDrag(panelRef, rect, screen, minPanel)
  // The map window is moved and resized by its own frame.
  const floating = !fills
  const toggleFill = (e: ReactMouseEvent) => {
    if (!(e.target as HTMLElement).closest(BAR_CONTROLS)) useSettings.getState().setUi({ mapMaximized: !fills })
  }
  const mode3d = use3DMode()
  const data3d = useMap3D()
  // The same menu as on the map, from a pin in a list.
  const openPinMenu = (pin: Pin, e: ReactMouseEvent) => {
    e.preventDefault()
    const box = panelRef.current?.getBoundingClientRect()
    const map = useStore.getState().map
    if (box && map) setMenu({ lngLat: toLngLat(map.projection, pin.position), x: e.clientX - box.left, y: e.clientY - box.top, pinId: pin.id })
  }

  return (
    <div className="relative h-full w-full" onMouseDown={closeMenu}>
      <div
        ref={panelRef}
        className="absolute overflow-hidden rounded-3xl bg-black/40 shadow-2xl ring-1 ring-white/10"
        style={{ ...rect, opacity, borderRadius: fills ? 0 : undefined }}
      >
        <MapView variant="full" onContextMenu={setMenu} fitPadding={fitPadding} />
        {mode3d === 'caves' && data3d && <Cave3DView data={data3d} onContextMenu={setMenu} />}

        {/* Interface layer, scaled for the screen size; the map underneath is not. Paused
            while the map is closed: it would keep up with every snapshot for nobody. */}
        <Activity mode={shown ? 'visible' : 'hidden'}>
        <div className="pointer-events-none absolute inset-0" style={{ zoom: uiScale }}>
          <div className="absolute inset-0 flex flex-col gap-3 p-3">
            {/* Above the sidebar and cards: its menus open over them. */}
            <div className="pointer-events-auto relative z-30">
              <TopBar
                onSettings={() => setSettingsOpen(!settingsOpen)}
                settingsOpen={settingsOpen}
                onMove={floating ? drag.move : undefined}
                onToggleFill={IS_MAP_WINDOW ? undefined : toggleFill}
              />
              <LinkProblem />
            </div>
            <div className="flex min-h-0 flex-1 gap-3">
              <div className="pointer-events-auto min-h-0">
                <Sidebar onEditPin={setEditing} onPinMenu={openPinMenu} />
              </div>
              <div className="flex min-w-0 flex-1 items-end justify-center">
                {mode3d === 'caves' && data3d && (
                  <div className="pointer-events-auto">
                    <Cave3DControls data={data3d} />
                  </div>
                )}
                {mode3d === 'surface' && (
                  <div className="pointer-events-auto">
                    <Surface3DControls />
                  </div>
                )}
              </div>
              <div className="flex min-h-0 min-w-0 flex-col items-end gap-3">
                {settingsOpen && <SettingsPanel onClose={() => setSettingsOpen(false)} />}
                {!settingsOpen && hasSelection && <PinDetails onEdit={setEditing} />}
                {!settingsOpen && hasLiveSelection && <LiveDetails />}
                <div className="flex-1" />
                <SyncRequests />
                {tracking && (
                  <div className="pointer-events-auto">
                    <Compass />
                  </div>
                )}
              </div>
            </div>
          </div>

          <MissingMap />
          <Download3D />
          <FogTip />
          {menu && <ContextMenu at={menu} scale={uiScale} onClose={closeMenu} onAddPin={setEditing} />}
          {editing && <PinEditor pin={editing} onClose={() => setEditing(null)} />}
          <Toast />
        </div>
        </Activity>
        {floating && !editingLayout && HANDLES.map((h) => <div key={h} className={`absolute z-40 touch-none hover:bg-accent/50 ${GRIP_STYLE[h]}`} onPointerDown={drag.resize(h)} />)}
      </div>
      {editingLayout && !IS_MAP_WINDOW && <LayoutEditor panelRef={panelRef} panel={rect} screen={screen} minPanel={minPanel} />}
    </div>
  )
}
