import { useEffect, useRef, useState } from 'react'
import { backend } from '../lib/backend'
import { formatDistance } from '../lib/compass'
import type { CameraView, GroundPoint, Vec3 } from '../lib/protocol'
import { breadcrumbs, type Crumb } from '../lib/route'
import { resolveTracked } from '../lib/tracking'
import { projectPoint, projectToScreen } from '../lib/waypoint'
import { useStore } from '../state/store'
import { useSettings } from '../settings/store'

/** Distance (meters) under which the waypoint fades out: the target is right there. */
const ARRIVED_METERS = 3
/** Breadcrumbs on the ground along the route: off for now, while they're reworked. */
const BREADCRUMBS = false
/** Breadcrumbs: how far ahead along the route, and how far apart (meters). */
const CRUMB_AHEAD_METERS = 40
const CRUMB_STEP_METERS = 2.5
/** How often breadcrumbs are snapped to the floor and checked for line of sight. */
const PROBE_INTERVAL_MS = 125
/** Arrow shape on the floor (meters): length from tail to tip, and width. */
const ARROW_LENGTH = 0.55
const ARROW_WIDTH = 0.6
/** Arrows float this far above the floor so it doesn't swallow them. */
const ARROW_LIFT = 0.06
/** The glow's delay from one arrow to the next (it runs toward the destination). */
const FLOW_STEP_S = 0.14

/**
 * The tracked target drawn over the game where it is in the world: a marker with its
 * name and distance, or an arrow at the screen edge when it's off screen or behind.
 * Needs the game camera (sent by the agent) and a HUD window covering the game.
 */
export function Waypoint() {
  const tracked = useStore((s) => s.tracked)
  const snapshot = useStore((s) => s.snapshot)
  const game = useStore((s) => s.game)
  const units = useSettings((s) => s.ui.units)
  const showRoute = useSettings((s) => s.ui.showRoute)
  const route = useStore((s) => (showRoute ? s.route : null))
  useStore((s) => s.user) // custom pin edits
  const local = snapshot?.local
  const camera = local?.camera
  const upm = game?.unitsPerMeter ?? 1
  const crumbs = BREADCRUMBS && route && local ? breadcrumbs(route.corners, local.position, CRUMB_AHEAD_METERS * upm, CRUMB_STEP_METERS * upm) : []
  const ground = useGround(crumbs)
  if (!tracked || !local || !camera || !game) return null
  const target = resolveTracked(tracked, snapshot)
  if (!target.position) return null

  const t = target.position
  const p = local.position
  const meters = Math.hypot(t.x - p.x, t.y - p.y, t.z - p.z) / game.unitsPerMeter
  const screen = projectToScreen(camera, t, window.innerWidth, window.innerHeight)
  const opacity = meters < ARRIVED_METERS ? meters / ARRIVED_METERS : 1
  return (
    <>
      {crumbs.length > 0 && <Crumbs crumbs={crumbs} ground={ground} camera={camera} unitsPerMeter={game.unitsPerMeter} />}
      <div
        className="pointer-events-none absolute flex -translate-x-1/2 -translate-y-1/2 flex-col items-center gap-1 transition-opacity"
        style={{ left: screen.x, top: screen.y, opacity }}
      >
        {screen.onScreen ? (
          <svg width="26" height="34" viewBox="0 0 26 34" aria-hidden="true" className="drop-shadow-[0_2px_4px_rgba(0,0,0,0.7)]">
            <path d="M13 33 C13 33 2 19 2 12 A11 11 0 0 1 24 12 C24 19 13 33 13 33 Z" fill="var(--color-accent)" stroke="#fff" strokeWidth="2" />
            <circle cx="13" cy="12" r="4" fill="#fff" />
          </svg>
        ) : (
          <svg width="30" height="30" viewBox="0 0 30 30" aria-hidden="true" style={{ transform: `rotate(${screen.angle}deg)` }} className="drop-shadow-[0_2px_4px_rgba(0,0,0,0.7)]">
            <path d="M15 2 L26 26 L15 20 L4 26 Z" fill="var(--color-accent)" stroke="#fff" strokeWidth="2" strokeLinejoin="round" />
          </svg>
        )}
        <div className="whitespace-nowrap rounded-full bg-black/65 px-2.5 py-0.5 text-center text-[12px] font-semibold text-white shadow">
          {target.name} <span className="font-normal text-white/75">· {formatDistance(meters, units)}</span>
        </div>
      </div>
    </>
  )
}

/** What the game says about each breadcrumb's spot (by `Crumb.key`). */
interface Ground {
  points: Map<number, GroundPoint>
  /** False when the game can't probe (older agent): crumbs are drawn unchecked. */
  available: boolean
}

/**
 * Keeps asking the game for the floor under the breadcrumbs and whether the camera
 * sees them, at `PROBE_INTERVAL_MS` (one request at a time).
 */
function useGround(crumbs: Crumb[]): Ground {
  const latest = useRef(crumbs)
  useEffect(() => {
    latest.current = crumbs
  })
  const [ground, setGround] = useState<Ground>({ points: new Map(), available: true })
  useEffect(() => {
    let busy = false
    let cancelled = false
    const id = setInterval(async () => {
      const batch = latest.current
      if (busy || batch.length === 0) return
      busy = true
      try {
        const points = await backend.probeGround(batch.map((c) => c.position))
        if (!cancelled) setGround({ points: new Map(batch.map((c, i) => [c.key, points[i]])), available: true })
      } catch {
        if (!cancelled) setGround((g) => (g.available ? { points: new Map(), available: false } : g))
      } finally {
        busy = false
      }
    }, PROBE_INTERVAL_MS)
    return () => {
      cancelled = true
      clearInterval(id)
    }
  }, [])
  return ground
}

/**
 * Breadcrumbs: arrows lying on the floor along the route, pointing the way, with a glow
 * running toward the destination. Arrows the camera can't see (behind walls) are hidden.
 */
function Crumbs({ crumbs, ground, camera, unitsPerMeter }: { crumbs: Crumb[]; ground: Ground; camera: CameraView; unitsPerMeter: number }) {
  const w = window.innerWidth
  const h = window.innerHeight
  return (
    <svg className="pointer-events-none absolute inset-0" width={w} height={h} aria-hidden="true">
      {crumbs.map((c, i) => {
        const probe = ground.points.get(c.key)
        // Until the game has checked a spot, it may be behind a wall: wait.
        if (ground.available && (!probe || !probe.visible)) return null
        const y = (probe?.y ?? c.position.y) + ARROW_LIFT * unitsPerMeter
        const outline = arrowOutline({ ...c.position, y }, c.direction, unitsPerMeter)
        const screen = outline.map((p) => projectPoint(camera, p, w, h))
        if (screen.some((p) => p === null)) return null
        // Farther arrows fade a little, so the nearest read first.
        const fade = 1 - (i / Math.max(1, crumbs.length)) * 0.45
        return (
          <polygon
            key={c.key}
            className="crumb"
            points={screen.map((p) => `${p!.x.toFixed(1)},${p!.y.toFixed(1)}`).join(' ')}
            fill="var(--color-accent)"
            stroke="#fff"
            strokeOpacity={0.85}
            strokeWidth={1.5}
            strokeLinejoin="round"
            style={{ opacity: fade, animationDelay: `${(-c.key * FLOW_STEP_S).toFixed(2)}s`, filter: 'drop-shadow(0 1px 2px rgb(0 0 0 / 0.6))' }}
          />
        )
      })}
    </svg>
  )
}

/** A chevron lying flat on the floor at `at`, pointing along `direction` (world points). */
function arrowOutline(at: Vec3, direction: Vec3, unitsPerMeter: number): Vec3[] {
  const f = { x: direction.x * unitsPerMeter, z: direction.z * unitsPerMeter }
  const s = { x: -f.z, z: f.x }
  const point = (forward: number, side: number): Vec3 => ({ x: at.x + f.x * forward + s.x * side, y: at.y, z: at.z + f.z * forward + s.z * side })
  const half = ARROW_LENGTH / 2
  const wing = ARROW_WIDTH / 2
  return [point(half, 0), point(-half, wing), point(-half * 0.25, 0), point(-half, -wing)]
}
