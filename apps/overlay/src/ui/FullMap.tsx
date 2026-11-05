import { Check, Copy, Info, MapPinPlus, Navigation, Pencil, Radio, Trash2, Zap } from 'lucide-react'
import { Activity, useEffect, useLayoutEffect, useMemo, useRef, useState, type MouseEvent as ReactMouseEvent } from 'react'
import { findPin, newCustomPinAt, teleportToMapPoint } from '../lib/actions'
import type { Pin } from '../lib/game'
import { fitRect, MIN_MAP_PANEL, toPixels } from '../lib/layout'
import { toLngLat, toWorld } from '../lib/projection'
import { readyPeers, sendPing, useSyncStore } from '../lib/sync/sync'
import { useUiScale, useWindowSize } from '../lib/uiScale'
import { MapView, type MapContextMenuEvent } from '../map/MapView'
import { useCanTeleport, useTeleportOffered } from '../lib/teleport'
import { selectViewLayer, useStore } from '../state/store'
import { MissingMap } from './MissingMap'
import { FogTip } from './Onboarding'
import { Compass } from './Compass'
import { LayoutEditor } from './LayoutEditor'
import { LiveDetails } from './LiveDetails'
import { PinDetails } from './PinDetails'
import { PinEditor } from './PinEditor'
import { SettingsPanel } from './SettingsPanel'
import { Sidebar } from './Sidebar'
import { SyncRequests } from './SyncRequests'
import { LinkProblem, TopBar } from './TopBar'
import { useSettings } from '../settings/store'
import { IS_MAP_WINDOW } from '../lib/view'

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
  const world = map ? toWorld(map.projection, at.lngLat) : null
  const item = 'flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-[13px] hover:bg-panel-raised disabled:opacity-40'
  const pin = at.pinId ? findPin(at.pinId) : undefined
  // Up from the bottom of the map when it wouldn't fit there.
  const ref = useRef<HTMLDivElement>(null)
  const [lift, setLift] = useState(0)
  useLayoutEffect(() => {
    const el = ref.current
    const parent = el?.offsetParent
    if (el && parent) setLift(Math.max(0, el.getBoundingClientRect().height + at.y + 8 - parent.getBoundingClientRect().height))
  }, [at, pin])
  return (
    <div
      ref={ref}
      className="glass pointer-events-auto absolute z-20 w-[200px] rounded-xl p-1"
      style={{ left: at.x / scale, top: (at.y - lift) / scale }}
      onMouseDown={(e) => e.stopPropagation()}
      onContextMenu={(e) => e.preventDefault()}
    >
      {pin ? (
        <PinMenuItems pin={pin} item={item} onEdit={onAddPin} onClose={onClose} />
      ) : (
        <button className={item} onClick={() => (onAddPin(newCustomPinAt(at.lngLat)), onClose())}>
          <MapPinPlus className="size-4" /> Add pin here
        </button>
      )}
      {teleportOffered && (
        <button className={item} disabled={!canTeleport} onClick={() => (void teleportToMapPoint(at.lngLat), onClose())}>
          <Zap className="size-4" /> Teleport here
        </button>
      )}
      {together && world && (
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
  const found = useStore((s) => s.user.found.includes(pin.id))
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
    const t = setTimeout(() => dismiss(toast.id), 2600)
    return () => clearTimeout(t)
  }, [toast, dismiss])
  if (!toast) return null
  return (
    <div
      role="status"
      className={`glass pointer-events-none absolute bottom-5 left-1/2 z-40 -translate-x-1/2 rounded-full px-4 py-2 text-[13px] ${
        toast.tone === 'error' ? 'text-danger' : ''
      }`}
    >
      {toast.text}
    </div>
  )
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
  const rect = useMemo(() => {
    const r = fitRect(toPixels(mapPanel, screen), screen)
    return fills ? { left: 0, top: 0, ...screen } : fitRect({ ...r, width: Math.max(r.width, MIN_MAP_PANEL.width), height: Math.max(r.height, MIN_MAP_PANEL.height) }, screen)
  }, [mapPanel, screen, fills])
  const hasSelection = useStore((s) => s.selectedPinId !== null)
  const hasLiveSelection = useStore((s) => s.selectedLiveId !== null)
  const tracking = useStore((s) => s.tracked !== null)
  const [menu, setMenu] = useState<MapContextMenuEvent | null>(null)
  const [editing, setEditing] = useState<Pin | null>(null)
  const [settingsOpen, setSettingsOpen] = useState(false)
  const shown = useStore((s) => IS_MAP_WINDOW || s.mode === 'open')
  const uiScale = useUiScale()
  const fitPadding = useMemo(() => panelPadding(uiScale), [uiScale])
  // The same menu as on the map, from a pin in a list.
  const openPinMenu = (pin: Pin, e: ReactMouseEvent) => {
    e.preventDefault()
    const box = panelRef.current?.getBoundingClientRect()
    const map = useStore.getState().map
    if (box && map) setMenu({ lngLat: toLngLat(map.projection, pin.position), x: e.clientX - box.left, y: e.clientY - box.top, pinId: pin.id })
  }

  return (
    <div className="relative h-full w-full" onMouseDown={() => setMenu(null)}>
      <div
        ref={panelRef}
        className="absolute overflow-hidden rounded-3xl bg-black/40 shadow-2xl ring-1 ring-white/10"
        style={{ ...rect, opacity, borderRadius: fills ? 0 : undefined }}
      >
        <MapView variant="full" onContextMenu={setMenu} fitPadding={fitPadding} />

        {/* Interface layer, scaled for the screen size; the map underneath is not. Paused
            while the map is closed: it would keep up with every snapshot for nobody. */}
        <Activity mode={shown ? 'visible' : 'hidden'}>
        <div className="pointer-events-none absolute inset-0" style={{ zoom: uiScale }}>
          <div className="absolute inset-0 flex flex-col gap-3 p-3">
            {/* Above the sidebar and cards: its menus open over them. */}
            <div className="pointer-events-auto relative z-30">
              <TopBar onSettings={() => setSettingsOpen(!settingsOpen)} settingsOpen={settingsOpen} />
              <LinkProblem />
            </div>
            <div className="flex min-h-0 flex-1 gap-3">
              <div className="pointer-events-auto min-h-0">
                <Sidebar onEditPin={setEditing} onPinMenu={openPinMenu} />
              </div>
              <div className="flex-1" />
              <div className="flex min-h-0 flex-col items-end gap-3">
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
          <FogTip />
          {menu && <ContextMenu at={menu} scale={uiScale} onClose={() => setMenu(null)} onAddPin={setEditing} />}
          {editing && <PinEditor pin={editing} onClose={() => setEditing(null)} />}
          <Toast />
        </div>
        </Activity>
      </div>
      {editingLayout && !IS_MAP_WINDOW && <LayoutEditor panelRef={panelRef} panel={rect} screen={screen} />}
    </div>
  )
}
