import { ArrowDown, ArrowUp, X } from 'lucide-react'
import { compassReading, formatDistance, formatHeight, type Units } from '../lib/compass'
import { resolveTracked } from '../lib/tracking'
import { findPin } from '../lib/actions'
import { MOVE_COLORS, nextStep, pathLength, trackPath, type RouteStep } from '../lib/route'
import { useStore } from '../state/store'
import { useSettings } from '../settings/store'

const NEARBY_METERS = 4
const NEARBY_RISE_METERS = 3
/** A route this much longer than the straight line is worth saying. */
const DETOUR = 1.15
/** A climb, ride or swim this close (meters) is where the player is. */
const STEP_NOW_METERS = 3

/** What to do next on the route that isn't walking, and where: "Climb up the rope in 12 m". */
function stepText(step: RouteStep, units: Units, unitsPerMeter: number): string {
  const what =
    step.move === 'rope'
      ? `Climb ${step.rise >= 0 ? 'up' : 'down'} the rope`
      : step.move === 'zipline'
        ? 'Ride the zipline'
        : step.move === 'dive'
          ? `Dive ${formatDistance(step.length / unitsPerMeter, units)} underwater`
          : 'Swim'
  const at = step.at / unitsPerMeter
  return `${what} ${at < STEP_NOW_METERS ? 'now' : `in ${formatDistance(at, units)}`}`
}

/** Direction + distance to the tracked pin or player, relative to the camera. */
export function Compass({ compact = false, interactive = true }: { compact?: boolean; interactive?: boolean }) {
  const tracked = useStore((s) => s.tracked)
  const game = useStore((s) => s.game)
  const map = useStore((s) => s.map)
  const local = useStore((s) => s.snapshot?.local ?? null)
  const snapshot = useStore((s) => s.snapshot)
  const route = useStore((s) => s.route)
  useStore((s) => s.user) // custom pin edits
  const setTracked = useStore((s) => s.setTracked)
  const units = useSettings((s) => s.ui.units)
  if (!tracked || !game || !map) return null

  const { name, position: target } = resolveTracked(tracked, snapshot)

  const reading = local && target ? compassReading(map.projection, local, target, game.unitsPerMeter) : null
  // The walk along the route, and whether the route stops short of the target.
  const path = local && target && route ? trackPath(local.position, target, route, game.unitsPerMeter) : null
  const walk = path && path.walkable > 0 ? pathLength(path.points, path.walkable) / game.unitsPerMeter : null
  const short = !!path && path.walkable < path.points.length - 1
  // From the surface, routes to an underground target end at its entrance on purpose.
  const targetLayer = tracked.kind === 'pin' ? findPin(tracked.id)?.layer : tracked.kind === 'point' ? tracked.layer : undefined
  const viaEntrance = short && local?.layer !== 'caves' && targetLayer === 'caves'
  // Right under or over it (a ledge up a rope, a cave below) isn't there yet.
  const nearby = reading !== null && reading.distance < NEARBY_METERS && Math.abs(reading.rise) < NEARBY_RISE_METERS
  const step = path && !nearby ? nextStep(path) : null
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
              <span className="tabular-nums">{formatDistance(reading.distance, units)}</span>
              {walk !== null && !short && walk > reading.distance * DETOUR && (
                <span className="tabular-nums text-ink-faint" title="Along the route">
                  {formatDistance(walk, units)} by route
                </span>
              )}
              {Math.abs(reading.rise) >= 5 && (
                <span className="flex items-center tabular-nums text-ink-faint">
                  {reading.rise > 0 ? <ArrowUp className="size-3" /> : <ArrowDown className="size-3" />}
                  {formatHeight(Math.abs(reading.rise), units)}
                </span>
              )}
            </>
          )}
        </div>
        {step && reading && (
          <div className="flex items-center gap-1.5 text-[11px] text-ink-dim">
            <span className="size-2 shrink-0 rounded-full" style={{ background: MOVE_COLORS[step.move] }} aria-hidden="true" />
            {stepText(step, units, game.unitsPerMeter)}
          </div>
        )}
        {short && reading && !nearby && (
          <div
            className="text-[11px] text-ink-faint"
            title={viaEntrance ? 'The route leads to the way in; inside, it carries on from there' : "There's no walking route the rest of the way: it may take a climb, a rope or a jump"}
          >
            {viaEntrance ? 'Via the cave entrance' : 'Route ends short of it'}
          </div>
        )}
      </div>
      {interactive && (
        <button className="ml-1 self-start text-ink-faint hover:text-ink" onClick={() => setTracked(null)} aria-label="Stop tracking">
          <X className="size-4" />
        </button>
      )}
    </div>
  )
}
