import { Check, Navigation, Pencil, Send, Timer, Trash2, X, Zap } from 'lucide-react'
import { useEffect, useState } from 'react'
import { findPin, teleportToPin } from '../lib/actions'
import { compassReading, formatDistance } from '../lib/compass'
import { LAYER_LABELS, type Pin } from '../lib/game'
import { readyPeers, shareTrack, targetOfPin, useSyncStore } from '../lib/sync/sync'
import { formatRemaining } from '../lib/timers'
import { useCanTeleport, useTeleportOffered } from '../lib/teleport'
import { useStore } from '../state/store'
import { Button, ButtonGrid } from './controls'
import { CategoryIcon } from './Sidebar'
import { useSettings } from '../settings/store'

/** Countdowns offered on a pin, in minutes. */
const TIMER_MINUTES = [5, 15, 30, 60]

/** The player's own note and countdown on a pin (any pin, not just their own). */
function PinExtras({ pinId }: { pinId: string }) {
  const note = useStore((s) => s.user.notes[pinId] ?? '')
  const endsAt = useStore((s) => s.user.timers[pinId] ?? null)
  const { setPinNote, setPinTimer } = useStore.getState()
  const [draft, setDraft] = useState(note)
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    if (endsAt === null) return
    const id = setInterval(() => setNow(Date.now()), 1000)
    return () => clearInterval(id)
  }, [endsAt])
  return (
    <div className="mt-3 space-y-2">
      <textarea
        rows={2}
        value={draft}
        placeholder="Add a note…"
        aria-label="Your note on this pin"
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => draft !== note && setPinNote(pinId, draft.trim())}
        onKeyDown={(e) => e.stopPropagation()}
        className="w-full resize-none rounded-lg border border-line bg-black/30 px-2.5 py-1.5 text-[12.5px] outline-none focus:border-accent/60"
      />
      <div className="flex flex-wrap items-center gap-1.5 text-[11.5px] text-ink-faint">
        {endsAt !== null ? (
          <>
            <Timer className="size-3.5 text-accent" />
            <span className="tabular-nums text-ink">{formatRemaining(endsAt - now)}</span>
            <button className="rounded-md px-1.5 py-0.5 hover:bg-panel-raised hover:text-ink" onClick={() => setPinTimer(pinId, null)}>
              Stop
            </button>
          </>
        ) : (
          <>
            <Timer className="size-3.5" />
            {TIMER_MINUTES.map((m) => (
              <button key={m} className="rounded-md px-1.5 py-0.5 tabular-nums hover:bg-panel-raised hover:text-ink" title={`Remind me in ${m} minutes`} onClick={() => setPinTimer(pinId, Date.now() + m * 60_000)}>
                {m}m
              </button>
            ))}
          </>
        )}
      </div>
    </div>
  )
}

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
  const teleportOffered = useTeleportOffered()
  const canTeleport = useCanTeleport()
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
      <PinExtras key={pin.id} pinId={pin.id} />
      <ButtonGrid className="mt-4">
        <Button tone={isTracked ? 'primary' : 'default'} onClick={() => setTracked(isTracked ? null : { kind: 'pin', id: pin.id })}>
          <Navigation className="size-4" /> {isTracked ? 'Tracking' : 'Track'}
        </Button>
        {teleportOffered && (
          <Button disabled={!canTeleport} title={canTeleport ? undefined : 'Teleport needs the game running'} onClick={() => teleportToPin(pin)}>
            <Zap className="size-4" /> Teleport
          </Button>
        )}
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
        <Button tone={found ? 'primary' : 'default'} onClick={() => toggleFound(pin.id)}>
          <Check className="size-4" /> {found ? 'Found' : 'Mark found'}
        </Button>
        {pin.custom && (
          <>
            <Button onClick={() => onEdit(pin)}>
              <Pencil className="size-4" /> Edit
            </Button>
            <Button tone="danger" onClick={() => deleteCustomPin(pin.id)}>
              <Trash2 className="size-4" /> Delete
            </Button>
          </>
        )}
      </ButtonGrid>
      <p className="mt-3 font-mono text-[11px] text-ink-faint">
        {pin.position.x.toFixed(1)}, {pin.snapToGround ? '—' : pin.position.y.toFixed(1)}, {pin.position.z.toFixed(1)}
      </p>
    </div>
  )
}
