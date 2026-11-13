import { Crosshair, Layers, LocateFixed, Move, PictureInPicture2, Settings, Undo2, Users, X } from 'lucide-react'
import { useMemo, useState } from 'react'
import { centerOnPlayer, teleportBack } from '../lib/actions'
import { backend } from '../lib/backend'
import { compassReading, formatDistance } from '../lib/compass'
import { formatEnvironment } from '../lib/environment'
import { LAYER_LABELS } from '../lib/game'
import { hotkeyLabel } from '../lib/keys'
import { movers } from '../lib/movers'
import { useSyncStore } from '../lib/sync/sync'
import { CONNECTION_LABELS, connectionOf } from '../lib/together'
import { useCanTeleport } from '../lib/teleport'
import { selectViewLayer, useStore } from '../state/store'
import { use3DDownload, useMap3D, useView3D } from '../state/view3d'
import { IconButton } from './controls'
import { WorldMenu } from './WorldMenu'
import { LiveActions } from './LiveActions'
import { IS_MAP_WINDOW } from '../lib/view'
import { mapHotkey, useSettings } from '../settings/store'

/** A connection problem in full, under the top bar: too long and too useful to cut off. */
export function LinkProblem() {
  const status = useStore((s) => s.status)
  const failed = status?.state === 'error'
  const message = failed ? (status.message ?? 'Connection error') : status?.state === 'connected' ? status.message : null
  if (!message) return null
  return (
    <p className="glass mt-2 rounded-xl px-3 py-1.5 text-[12.5px] leading-snug break-words">
      {message}
      {failed && <span className="text-ink-dim"> The map still works, and live data comes back by itself.</span>}
    </p>
  )
}

function StatusChip() {
  const status = useStore((s) => s.status)
  const hasLocation = useStore((s) => !!s.snapshot?.local)
  const game = useStore((s) => s.game)
  const environment = useStore((s) => (s.snapshot?.environment ? formatEnvironment(s.snapshot.environment) : null))
  let tone = 'bg-white/30'
  let text = 'Start the game to see your location'
  if (status?.state === 'ready') {
    text = 'Not connected. Click Connect in the Game Maps window'
  } else if (status?.state === 'connecting') {
    tone = 'bg-amber-300 animate-pulse'
    text = 'Connecting to game…'
  } else if (status?.state === 'error') {
    tone = 'bg-danger'
    text = 'Live data lost'
  } else if (status?.state === 'connected') {
    tone = hasLocation ? 'bg-live' : 'bg-amber-300'
    text = status.message ? 'Connected, with a problem' : hasLocation ? 'Live' : 'Load into a world to see your location'
  }
  return (
    <div className="flex min-w-0 items-center gap-2 px-1" title={text}>
      <span className={`size-2 shrink-0 rounded-full ${tone}`} />
      <span className="font-display shrink-0 truncate text-[14px] font-semibold">{game?.name ?? 'Game Maps'}</span>
      <WorldMenu />
      <span className="truncate text-[12px] text-ink-faint">{text}</span>
      {environment && <span className="shrink-0 text-[12px] tabular-nums text-ink-dim">{environment}</span>}
    </div>
  )
}

// Stable empty value: selectors must not return fresh arrays (infinite re-render).
function PlayersMenu() {
  const snapshot = useStore((s) => s.snapshot)
  const others = useMemo(() => movers(snapshot), [snapshot])
  const local = useStore((s) => s.snapshot?.local ?? null)
  const map = useStore((s) => s.map)
  const game = useStore((s) => s.game)
  const units = useSettings((s) => s.ui.units)
  const selectLive = useStore((s) => s.selectLive)
  const [open, setOpen] = useState(false)
  const peers = useSyncStore((s) => s.peers)
  const inRoom = useSyncStore((s) => s.session !== null)
  if (others.length === 0) return null
  return (
    <div className="relative">
      <IconButton label="Players & companions" active={open} onClick={() => setOpen(!open)}>
        <span className="relative">
          <Users className="size-[18px]" />
          <span className="absolute -right-2 -top-1.5 rounded-full bg-live px-1 text-[9px] font-bold text-black">{others.length}</span>
        </span>
      </IconButton>
      {open && (
        <div className="glass absolute right-0 top-11 z-20 w-[280px] rounded-xl p-1.5">
          {others.map((o) => {
            const r = local && map && game ? compassReading(map.projection, local, o.position, game.unitsPerMeter) : null
            const connection = o.kind === 'player' && inRoom ? connectionOf(o, Object.values(peers)) : null
            return (
              <div key={o.id} className="flex items-center gap-2 rounded-lg px-2 py-1.5 hover:bg-panel-raised">
                <button
                  type="button"
                  className="flex min-w-0 flex-1 items-center gap-2 text-left"
                  title="Show details"
                  onClick={() => {
                    selectLive(o.id)
                    setOpen(false)
                  }}
                >
                  <span className="size-3 shrink-0 rounded-full ring-2 ring-white" style={{ background: o.color }} />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-[13px]">{o.name}</span>
                    {r && <span className="block text-[11px] text-ink-faint">{formatDistance(r.distance, units)}</span>}
                    {connection && (
                      <span className={`block text-[11px] ${connection === 'connected' ? 'text-live' : connection === 'outdated' ? 'text-danger' : 'text-ink-faint'}`}>{CONNECTION_LABELS[connection]}</span>
                    )}
                  </span>
                </button>
                <LiveActions entity={o} compact />
              </div>
            )
          })}
        </div>
      )}
    </div>
  )
}

export function TopBar({ onSettings, settingsOpen }: { onSettings: () => void; settingsOpen: boolean }) {
  const gameMap = useStore((s) => s.map)
  const follow = useStore((s) => s.followPlayer)
  const hud = useSettings((s) => s.ui.hud)
  const game = useStore((s) => s.game)
  const hotkey = useSettings((s) => mapHotkey(s.app, game))
  const setFollowPlayer = useStore((s) => s.setFollowPlayer)
  const setHud = useSettings((s) => s.setHud)
  const hasLocation = useStore((s) => !!s.snapshot?.local)
  const viewLayer = useStore(selectViewLayer)
  const override = useStore((s) => s.viewLayerOverride)
  const setViewLayer = useStore((s) => s.setViewLayer)
  const hasCaves = gameMap?.layers.includes('caves') ?? false
  const backSteps = useStore((s) => s.backSteps)
  const backKey = useSettings((s) => s.app?.back_key ?? null)
  const canTeleport = useCanTeleport()
  const data3d = useMap3D()
  const download3d = use3DDownload()
  // Offered once there's 3D data to show or to download.
  const has3d = data3d !== null || download3d.state === 'available' || download3d.state === 'downloading'
  const map3d = useSettings((s) => s.ui.map3d)
  const setUi = useSettings((s) => s.setUi)
  const setOffer = useView3D((s) => s.setOffer)
  // 3D without the data asks before downloading it (see `Download3D`).
  const pick3d = (on: boolean) => {
    if (on && data3d === null && download3d.state === 'available') return setOffer(true)
    setOffer(false)
    setUi({ map3d: on })
  }

  return (
    <header className="glass flex h-12 items-center gap-1 rounded-2xl px-2">
      <StatusChip />
      <div className="flex-1" />
      {hasCaves && (
        <div className="mr-1 flex items-center rounded-lg bg-black/30 p-0.5 text-[12px]" title={override ? 'Layer pinned' : 'Following your layer'}>
          <Layers className="mx-1.5 size-3.5 text-ink-faint" />
          {(['surface', 'caves'] as const).map((l) => (
            <button
              key={l}
              className={`rounded-md px-2 py-1 ${viewLayer === l ? 'bg-white/15 text-ink' : 'text-ink-dim hover:text-ink'}`}
              onClick={() => setViewLayer(override === l ? null : l)}
            >
              {LAYER_LABELS[l]}
            </button>
          ))}
        </div>
      )}
      {has3d && (
        <div className="mr-1 flex items-center rounded-lg bg-black/30 p-0.5 text-[12px]" role="radiogroup" aria-label="Map view">
          {([false, true] as const).map((on) => (
            <button
              key={String(on)}
              role="radio"
              aria-checked={map3d === on}
              title={on ? 'Tilt the terrain; see caves and bunkers as 3D models' : 'The flat map'}
              className={`rounded-md px-2 py-1 ${map3d === on ? 'bg-white/15 text-ink' : 'text-ink-dim hover:text-ink'}`}
              onClick={() => pick3d(on)}
            >
              {on ? '3D' : '2D'}
            </button>
          ))}
        </div>
      )}
      {canTeleport && (
        <IconButton
          label={backSteps > 0 ? `Teleport back to where you were${backKey ? ` (${hotkeyLabel(backKey)})` : ''}` : 'Teleport back (after a teleport)'}
          disabled={backSteps === 0}
          onClick={() => void teleportBack()}
        >
          <Undo2 className="size-[18px]" />
        </IconButton>
      )}
      <PlayersMenu />
      <IconButton label="Center on me" disabled={!hasLocation} onClick={centerOnPlayer}>
        <LocateFixed className="size-[18px]" />
      </IconButton>
      <IconButton label={follow ? 'Stop following' : 'Follow me'} active={follow} disabled={!hasLocation} onClick={() => setFollowPlayer(!follow)}>
        <Crosshair className="size-[18px]" />
      </IconButton>
      <IconButton label={hud.minimap ? 'Hide mini-map while playing' : 'Show mini-map while playing'} active={hud.minimap} onClick={() => setHud({ minimap: !hud.minimap })}>
        <PictureInPicture2 className="size-[18px]" />
      </IconButton>
      {!IS_MAP_WINDOW && (
        <IconButton label="Move and resize the map and mini-map" onClick={() => useStore.getState().setEditingLayout(true)}>
          <Move className="size-[18px]" />
        </IconButton>
      )}
      <IconButton label="Settings" active={settingsOpen} onClick={onSettings}>
        <Settings className="size-[18px]" />
      </IconButton>
      {!IS_MAP_WINDOW && (
        <>
          <div className="mx-1 h-6 w-px bg-line" />
          <IconButton label={`Close${hotkey ? ` (${hotkeyLabel(hotkey)})` : ''}`} onClick={() => void backend.closeOverlay()}>
            <X className="size-[18px]" />
          </IconButton>
        </>
      )}
    </header>
  )
}
