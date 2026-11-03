import { Crosshair, Navigation, X, Zap } from 'lucide-react'
import { teleportToPlayer } from '../lib/actions'
import { compassReading, formatDistance } from '../lib/compass'
import { iconUrl } from '../lib/game'
import { findMover, MOVER_KIND_LABELS } from '../lib/movers'
import { useStore } from '../state/store'
import { Button } from './controls'
import { useSettings } from '../settings/store'

/** Details of the selected live entity (player, companion, golf cart, ...) with its actions. */
export function LiveDetails() {
  const id = useStore((s) => s.selectedLiveId)
  const snapshot = useStore((s) => s.snapshot)
  const game = useStore((s) => s.game)
  const map = useStore((s) => s.map)
  const tracked = useStore((s) => s.tracked)
  const followId = useStore((s) => s.followId)
  const units = useSettings((s) => s.ui.units)
  const canTeleport = useStore((s) => s.status?.state === 'connected' && !!s.status.capabilities?.teleport)
  const { selectLive, setTracked, setFollow } = useStore.getState()

  const entity = id ? findMover(snapshot, id) : undefined
  if (!id || !game || !map) return null
  if (!entity) {
    // Gone from the game (left, despawned, or out of range): keep the card until closed.
    return (
      <div className="glass pointer-events-auto flex w-[340px] items-center gap-3 rounded-2xl p-4">
        <p className="flex-1 text-[13px] text-ink-dim">No longer on the map.</p>
        <button className="text-ink-faint hover:text-ink" onClick={() => selectLive(null)} aria-label="Close">
          <X className="size-4" />
        </button>
      </div>
    )
  }
  const local = snapshot?.local ?? null
  const reading = local ? compassReading(map.projection, local, entity.position, game.unitsPerMeter) : null
  const isTracked = tracked?.kind === 'live' && tracked.id === entity.id
  const isFollowed = followId === entity.id
  const icon = entity.icon ? iconUrl(game.id, `game:${entity.icon}`) : null

  return (
    <div className="glass pointer-events-auto w-[340px] rounded-2xl p-4">
      <div className="flex items-start gap-3">
        <span className="grid size-9 shrink-0 place-items-center rounded-full ring-2 ring-white/80" style={{ background: entity.color }}>
          {icon && <img src={icon} alt="" className="size-5 object-contain" />}
        </span>
        <div className="min-w-0 flex-1">
          <h2 className="font-display text-[16px] font-semibold leading-tight">{entity.name}</h2>
          <p className="text-[12px] text-ink-faint">
            {MOVER_KIND_LABELS[entity.kind]}
            {reading ? ` · ${formatDistance(reading.distance, units)} away` : ''}
          </p>
        </div>
        <button className="text-ink-faint hover:text-ink" onClick={() => selectLive(null)} aria-label="Close">
          <X className="size-4" />
        </button>
      </div>
      <div className="mt-4 flex flex-wrap gap-2">
        <Button tone={isFollowed ? 'primary' : 'default'} title="Keep the map centered on them" onClick={() => setFollow(isFollowed ? null : entity.id)}>
          <Crosshair className="size-4" /> {isFollowed ? 'Following' : 'Follow'}
        </Button>
        <Button tone={isTracked ? 'primary' : 'default'} title="Point the compass at them while you play" onClick={() => setTracked(isTracked ? null : { kind: 'live', id: entity.id })}>
          <Navigation className="size-4" /> {isTracked ? 'Tracking' : 'Track'}
        </Button>
        <Button disabled={!canTeleport} title={canTeleport ? undefined : 'Teleport needs the game running'} onClick={() => teleportToPlayer(entity.id, entity.name)}>
          <Zap className="size-4" /> Teleport
        </Button>
      </div>
    </div>
  )
}
