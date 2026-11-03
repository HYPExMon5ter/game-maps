import { Crosshair, Layers, LocateFixed, Navigation, PictureInPicture2, Settings, Undo2, Users, X } from 'lucide-react'
import { useMemo, useState } from 'react'
import { centerOnPlayer, teleportBack, teleportToPlayer } from '../lib/actions'
import { backend } from '../lib/backend'
import { compassReading, formatDistance } from '../lib/compass'
import { LAYER_LABELS } from '../lib/game'
import { hotkeyLabel } from '../lib/keys'
import { movers } from '../lib/movers'
import { selectViewLayer, useStore } from '../state/store'
import { Button, IconButton } from './controls'
import { mapHotkey, useSettings } from '../settings/store'

function StatusChip() {
  const status = useStore((s) => s.status)
  const hasLocation = useStore((s) => !!s.snapshot?.local)
  const game = useStore((s) => s.game)
  let tone = 'bg-white/30'
  let text = 'Start the game to see your location'
  if (status?.state === 'ready') {
    text = 'Not connected. Click Connect in the Game Maps window'
  } else if (status?.state === 'connecting') {
    tone = 'bg-amber-300 animate-pulse'
    text = 'Connecting to game…'
  } else if (status?.state === 'error') {
    tone = 'bg-danger'
    text = status.message ?? 'Connection error'
  } else if (status?.state === 'connected') {
    tone = hasLocation ? 'bg-live' : 'bg-amber-300'
    text = status.message ?? (hasLocation ? 'Live' : 'Load into a world to see your location')
  }
  return (
    <div className="flex min-w-0 items-center gap-2 px-1" title={text}>
      <span className={`size-2 shrink-0 rounded-full ${tone}`} />
      <span className="font-display truncate text-[14px] font-semibold">{game?.name ?? 'Game Maps'}</span>
      <span className="truncate text-[12px] text-ink-faint">{text}</span>
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
  const tracked = useStore((s) => s.tracked)
  const setTracked = useStore((s) => s.setTracked)
  const followId = useStore((s) => s.followId)
  const units = useSettings((s) => s.ui.units)
  const setFollow = useStore((s) => s.setFollow)
  const canTeleport = useStore((s) => !!s.status?.capabilities?.teleport)
  const [open, setOpen] = useState(false)
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
            const isTracked = tracked?.kind === 'live' && tracked.id === o.id
            const isFollowed = followId === o.id
            return (
              <div key={o.id} className="flex items-center gap-2 rounded-lg px-2 py-1.5 hover:bg-panel-raised">
                <span className="size-3 rounded-full ring-2 ring-white" style={{ background: o.color }} />
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-[13px]">{o.name}</span>
                  {r && <span className="block text-[11px] text-ink-faint">{formatDistance(r.distance, units)}</span>}
                </span>
                <Button tone={isTracked ? 'primary' : 'default'} className="!px-2" title="Track" onClick={() => setTracked(isTracked ? null : { kind: 'live', id: o.id })}>
                  <Navigation className="size-3.5" />
                </Button>
                <Button tone={isFollowed ? 'primary' : 'default'} className="!px-2" title={isFollowed ? 'Stop following' : 'Follow on the map'} onClick={() => setFollow(isFollowed ? null : o.id)}>
                  <Crosshair className="size-3.5" />
                </Button>
                {canTeleport && (
                  <Button className="!px-2" title={`Teleport to ${o.name}`} onClick={() => teleportToPlayer(o.id, o.name)}>
                    Go
                  </Button>
                )}
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
  const canTeleport = useStore((s) => s.status?.state === 'connected' && !!s.status.capabilities?.teleport)

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
      <IconButton label="Settings" active={settingsOpen} onClick={onSettings}>
        <Settings className="size-[18px]" />
      </IconButton>
      <div className="mx-1 h-6 w-px bg-line" />
      <IconButton label={`Close${hotkey ? ` (${hotkeyLabel(hotkey)})` : ''}`} onClick={() => void backend.closeOverlay()}>
        <X className="size-[18px]" />
      </IconButton>
    </header>
  )
}
