import { Check, Navigation, Pencil, Send, Trash2, X, Zap } from 'lucide-react'
import { findPin, teleportToPin } from '../lib/actions'
import { compassReading, formatDistance } from '../lib/compass'
import { LAYER_LABELS, type Pin } from '../lib/game'
import { readyPeers, shareTrack, targetOfPin, useSyncStore } from '../lib/sync/sync'
import { useStore } from '../state/store'
import { Button } from './controls'
import { CategoryIcon } from './Sidebar'
import { useSettings } from '../settings/store'

export function PinDetails({ onEdit }: { onEdit: (pin: Pin) => void }) {
  const id = useStore((s) => s.selectedPinId)
  const game = useStore((s) => s.game)
  const map = useStore((s) => s.map)
  // Re-render when pins change (edits, found state).
  useStore((s) => s.user)
  const found = useStore((s) => (id ? s.user.found.includes(id) : false))
  const tracked = useStore((s) => s.tracked)
  const units = useSettings((s) => s.ui.units)
  const local = useStore((s) => s.snapshot?.local ?? null)
  const canTeleport = useStore((s) => s.status?.state === 'connected' && !!s.status.capabilities?.teleport)
  const together = useSyncStore((s) => readyPeers(s).length > 0)
  const { selectPin, toggleFound, setTracked, deleteCustomPin } = useStore.getState()

  const pin = id ? findPin(id) : undefined
  if (!pin || !game || !map) return null
  const category = game.categories.find((c) => c.id === pin.category)
  const isTracked = tracked?.kind === 'pin' && tracked.id === pin.id
  const reading = local ? compassReading(map.projection, local, pin.position, game.unitsPerMeter) : null

  return (
    <div className="glass pointer-events-auto w-[340px] rounded-2xl p-4">
      <div className="flex items-start gap-3">
        <CategoryIcon gameId={game.id} category={category} icon={pin.icon} color={pin.color} size={36} />
        <div className="min-w-0 flex-1">
          <h2 className="font-display text-[16px] font-semibold leading-tight">{pin.name}</h2>
          <p className="text-[12px] text-ink-faint">
            {category?.name ?? pin.category}
            {pin.layer && pin.layer !== 'surface' ? ` · ${LAYER_LABELS[pin.layer]}` : ''}
            {reading ? ` · ${formatDistance(reading.distance, units)} away` : ''}
          </p>
        </div>
        <button className="text-ink-faint hover:text-ink" onClick={() => selectPin(null)} aria-label="Close">
          <X className="size-4" />
        </button>
      </div>
      {pin.description && <p className="mt-3 whitespace-pre-wrap text-[13px] leading-relaxed text-ink-dim">{pin.description}</p>}
      <div className="mt-4 flex flex-wrap gap-2">
        <Button tone={isTracked ? 'primary' : 'default'} onClick={() => setTracked(isTracked ? null : { kind: 'pin', id: pin.id })}>
          <Navigation className="size-4" /> {isTracked ? 'Tracking' : 'Track'}
        </Button>
        {together && (
          <Button
            title="Ask the other players to track this too"
            onClick={() => {
              const target = targetOfPin(pin.id)
              if (target) shareTrack(target)
            }}
          >
            <Send className="size-4" /> Share
          </Button>
        )}
        <Button disabled={!canTeleport} title={canTeleport ? undefined : 'Teleport needs the game running'} onClick={() => teleportToPin(pin)}>
          <Zap className="size-4" /> Teleport
        </Button>
        <Button tone={found ? 'primary' : 'default'} onClick={() => toggleFound(pin.id)}>
          <Check className="size-4" /> {found ? 'Found' : 'Mark found'}
        </Button>
        {pin.custom && (
          <>
            <Button onClick={() => onEdit(pin)}>
              <Pencil className="size-4" /> Edit
            </Button>
            <Button tone="danger" onClick={() => deleteCustomPin(pin.id)} aria-label="Delete pin">
              <Trash2 className="size-4" />
            </Button>
          </>
        )}
      </div>
      <p className="mt-3 font-mono text-[11px] text-ink-faint">
        {pin.position.x.toFixed(1)}, {pin.snapToGround ? '—' : pin.position.y.toFixed(1)}, {pin.position.z.toFixed(1)}
      </p>
    </div>
  )
}
