import { Copy, MapPinPlus, Radio, Zap } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { newCustomPinAt, teleportToMapPoint } from '../lib/actions'
import type { Pin } from '../lib/game'
import { toWorld } from '../lib/projection'
import { readyPeers, sendPing, useSyncStore } from '../lib/sync/sync'
import { useUiScale } from '../lib/uiScale'
import { MapView, type MapContextMenuEvent } from '../map/MapView'
import { selectViewLayer, useStore } from '../state/store'
import { Compass } from './Compass'
import { LiveDetails } from './LiveDetails'
import { PinDetails } from './PinDetails'
import { PinEditor } from './PinEditor'
import { SettingsPanel } from './SettingsPanel'
import { Sidebar } from './Sidebar'
import { SyncRequests } from './SyncRequests'
import { TopBar } from './TopBar'
import { useSettings } from '../settings/store'

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
  const canTeleport = useStore((s) => s.status?.state === 'connected' && !!s.status.capabilities?.teleport)
  const map = useStore((s) => s.map)
  const notify = useStore((s) => s.notify)
  const layer = useStore(selectViewLayer)
  const together = useSyncStore((s) => readyPeers(s).length > 0)
  const world = map ? toWorld(map.projection, at.lngLat) : null
  const item = 'flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-[13px] hover:bg-panel-raised disabled:opacity-40'
  return (
    <div
      className="glass pointer-events-auto absolute z-20 w-[200px] rounded-xl p-1"
      style={{ left: at.x / scale, top: at.y / scale }}
      onMouseDown={(e) => e.stopPropagation()}
    >
      <button className={item} onClick={() => (onAddPin(newCustomPinAt(at.lngLat)), onClose())}>
        <MapPinPlus className="size-4" /> Add pin here
      </button>
      <button className={item} disabled={!canTeleport} onClick={() => (void teleportToMapPoint(at.lngLat), onClose())}>
        <Zap className="size-4" /> Teleport here
      </button>
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

export function FullMap() {
  const opacity = useSettings((s) => s.ui.opacity)
  const scale = useSettings((s) => s.ui.panelScale)
  const hasSelection = useStore((s) => s.selectedPinId !== null)
  const hasLiveSelection = useStore((s) => s.selectedLiveId !== null)
  const tracking = useStore((s) => s.tracked !== null)
  const [menu, setMenu] = useState<MapContextMenuEvent | null>(null)
  const [editing, setEditing] = useState<Pin | null>(null)
  const [settingsOpen, setSettingsOpen] = useState(false)
  const uiScale = useUiScale()
  const fitPadding = useMemo(() => panelPadding(uiScale), [uiScale])

  return (
    <div className="flex h-full w-full items-center justify-center" onMouseDown={() => setMenu(null)}>
      <div
        className="relative overflow-hidden rounded-3xl bg-black/40 shadow-2xl ring-1 ring-white/10"
        style={{ width: `${scale * 100}%`, height: `${scale * 100}%`, opacity, borderRadius: scale >= 1 ? 0 : undefined }}
      >
        <MapView variant="full" onContextMenu={setMenu} fitPadding={fitPadding} />

        {/* Interface layer, scaled for the screen size; the map underneath is not. */}
        <div className="pointer-events-none absolute inset-0" style={{ zoom: uiScale }}>
          <div className="absolute inset-0 flex flex-col gap-3 p-3">
            <div className="pointer-events-auto">
              <TopBar onSettings={() => setSettingsOpen(!settingsOpen)} settingsOpen={settingsOpen} />
            </div>
            <div className="flex min-h-0 flex-1 gap-3">
              <div className="pointer-events-auto min-h-0">
                <Sidebar onEditPin={setEditing} />
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

          {menu && <ContextMenu at={menu} scale={uiScale} onClose={() => setMenu(null)} onAddPin={setEditing} />}
          {editing && <PinEditor pin={editing} onClose={() => setEditing(null)} />}
          <Toast />
        </div>
      </div>
    </div>
  )
}
