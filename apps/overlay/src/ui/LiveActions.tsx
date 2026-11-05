// Actions on a live entity (player, companion, live marker), the same everywhere they
// appear: Track, Follow, Teleport — the order pins use too (Track, Teleport, ...).

import { Crosshair, Navigation, Zap } from 'lucide-react'
import { teleportToPlayer } from '../lib/actions'
import type { Mover } from '../lib/movers'
import { useCanTeleport, useTeleportOffered } from '../lib/teleport'
import { useStore } from '../state/store'
import { Button } from './controls'

/** `compact`: icon-only buttons (lists); otherwise with labels (details cards). */
export function LiveActions({ entity, compact = false }: { entity: Mover; compact?: boolean }) {
  const isTracked = useStore((s) => s.tracked?.kind === 'live' && s.tracked.id === entity.id)
  const isFollowed = useStore((s) => s.followId === entity.id)
  const teleportOffered = useTeleportOffered()
  const canTeleport = useCanTeleport()
  const { setTracked, setFollow } = useStore.getState()
  const size = compact ? 'size-3.5' : 'size-4'
  const button = compact ? '!px-2' : ''
  const label = (text: string) => (compact ? null : ` ${text}`)

  return (
    <>
      <Button
        tone={isTracked ? 'primary' : 'default'}
        className={button}
        title={isTracked ? 'Stop tracking' : 'Track: point the compass and waypoint at them while you play'}
        aria-label={isTracked ? 'Stop tracking' : 'Track'}
        onClick={() => setTracked(isTracked ? null : { kind: 'live', id: entity.id })}
      >
        <Navigation className={size} />
        {label(isTracked ? 'Tracking' : 'Track')}
      </Button>
      <Button
        tone={isFollowed ? 'primary' : 'default'}
        className={button}
        title={isFollowed ? 'Stop following' : 'Follow: keep the map centered on them'}
        aria-label={isFollowed ? 'Stop following' : 'Follow'}
        onClick={() => setFollow(isFollowed ? null : entity.id)}
      >
        <Crosshair className={size} />
        {label(isFollowed ? 'Following' : 'Follow')}
      </Button>
      {teleportOffered && (
        <Button
          className={button}
          disabled={!canTeleport}
          title={canTeleport ? `Teleport to ${entity.name}` : 'Teleport needs the game running'}
          aria-label={`Teleport to ${entity.name}`}
          onClick={() => teleportToPlayer(entity.id, entity.name)}
        >
          <Zap className={size} />
          {label('Teleport')}
        </Button>
      )}
    </>
  )
}
