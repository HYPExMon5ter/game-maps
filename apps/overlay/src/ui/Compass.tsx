import { ArrowDown, ArrowUp, X } from 'lucide-react'
import { findPin } from '../lib/actions'
import { compassReading, formatDistance } from '../lib/compass'
import { findMover } from '../lib/movers'
import type { Vec3 } from '../lib/protocol'
import { useStore } from '../state/store'

const NEARBY_METERS = 4

/** Direction + distance to the tracked pin or player, relative to the camera. */
export function Compass({ compact = false, interactive = true }: { compact?: boolean; interactive?: boolean }) {
  const tracked = useStore((s) => s.tracked)
  const game = useStore((s) => s.game)
  const map = useStore((s) => s.map)
  const local = useStore((s) => s.snapshot?.local ?? null)
  const snapshot = useStore((s) => s.snapshot)
  useStore((s) => s.user) // custom pin edits
  const setTracked = useStore((s) => s.setTracked)
  if (!tracked || !game || !map) return null

  let name = ''
  let target: Vec3 | undefined
  if (tracked.kind === 'pin') {
    const pin = findPin(tracked.id)
    name = pin?.name ?? ''
    target = pin?.position
  } else {
    const p = findMover(snapshot, tracked.id)
    name = p?.name ?? 'Player'
    target = p?.position
  }

  const reading = local && target ? compassReading(map.projection, local, target, game.unitsPerMeter) : null
  const nearby = reading !== null && reading.distance < NEARBY_METERS
  // Without a camera direction, fall back to a north-up arrow.
  const angle = reading ? (reading.relative ?? reading.bearing) : null
  const dial = compact ? 52 : 76

  return (
    <div className={`glass flex items-center gap-3 rounded-2xl ${compact ? 'p-2 pr-3' : 'p-3 pr-4'}`}>
      <div className="relative grid shrink-0 place-items-center rounded-full border border-white/15 bg-white/5" style={{ width: dial, height: dial }}>
        {nearby ? (
          <span className="size-1/2 rounded-full bg-accent shadow-[0_0_18px_var(--color-accent)]" />
        ) : angle !== null ? (
          <svg viewBox="0 0 40 40" className="size-[78%] transition-transform duration-150" style={{ transform: `rotate(${angle}deg)` }} aria-hidden="true">
            <path d="M20 3 L31 33 L20 26 L9 33 Z" fill="var(--color-accent)" stroke="#fff" strokeWidth="1.5" strokeLinejoin="round" />
          </svg>
        ) : (
          <span className="text-[10px] text-ink-faint">…</span>
        )}
        {reading?.relative === null && !nearby && <span className="absolute -top-0.5 text-[9px] font-bold text-ink-faint">N</span>}
      </div>
      <div className="min-w-0">
        <div className={`truncate font-semibold ${compact ? 'max-w-[150px] text-[13px]' : 'max-w-[200px] text-[15px]'}`}>{name}</div>
        <div className="flex items-center gap-2 text-[12px] text-ink-dim">
          {!local ? (
            'Waiting for location…'
          ) : !reading ? (
            'Target unavailable'
          ) : nearby ? (
            'You are here'
          ) : (
            <>
              <span className="tabular-nums">{formatDistance(reading.distance)}</span>
              {Math.abs(reading.rise) >= 5 && (
                <span className="flex items-center tabular-nums text-ink-faint">
                  {reading.rise > 0 ? <ArrowUp className="size-3" /> : <ArrowDown className="size-3" />}
                  {Math.round(Math.abs(reading.rise))} m
                </span>
              )}
            </>
          )}
        </div>
      </div>
      {interactive && (
        <button className="ml-1 self-start text-ink-faint hover:text-ink" onClick={() => setTracked(null)} aria-label="Stop tracking">
          <X className="size-4" />
        </button>
      )}
    </div>
  )
}
