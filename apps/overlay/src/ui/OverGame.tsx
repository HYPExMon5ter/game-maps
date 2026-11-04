// Things drawn over the game itself (not the corner HUD): map pings from the session and
// the danger alert. Like the waypoint, they need the game camera and a HUD window that
// covers the game's screen, which it only does while one of them has something to show.

import { useEffect, useState } from 'react'
import { formatDistance } from '../lib/compass'
import { nearbyHostiles, type Threat } from '../lib/danger'
import { DANGER_EDGE, PING_EDGE, screenPoint } from '../lib/overGame'
import { PING_MS, useSyncStore, type Ping } from '../lib/sync/sync'
import { useGameSetting, useSettings } from '../settings/store'
import { useStore } from '../state/store'

/** The time, updated every `ms` while `active` (for fading). */
function useNow(active: boolean, ms: number): number {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    if (!active) return
    const id = setInterval(() => setNow(Date.now()), ms)
    return () => clearInterval(id)
  }, [active, ms])
  return now
}

/** Pings over the game, fading out over 30 seconds. */
export function PingMarkers() {
  const snapshot = useStore((s) => s.snapshot)
  const game = useStore((s) => s.game)
  const enabled = useGameSetting(game?.id, 'pingsInGame')
  const pings = useSyncStore((s) => s.pings)
  const units = useSettings((s) => s.ui.units)
  const now = useNow(pings.length > 0, 1000)
  if (!enabled || !snapshot?.local || !game) return null
  return pings.map((ping: Ping) => {
    const screen = screenPoint(snapshot, ping.position, PING_EDGE)
    if (!screen) return null
    const local = snapshot.local!.position
    const meters = Math.hypot(ping.position.x - local.x, ping.position.y - local.y, ping.position.z - local.z) / game.unitsPerMeter
    const fade = Math.max(0.2, 1 - (now - ping.at) / PING_MS)
    return (
      <div
        key={ping.id}
        className="pointer-events-none absolute flex -translate-x-1/2 -translate-y-1/2 flex-col items-center gap-1"
        style={{ left: screen.x, top: screen.y, opacity: fade }}
      >
        {screen.onScreen ? (
          <span className="relative grid size-7 place-items-center">
            <span className="absolute inset-0 animate-ping rounded-full bg-[#56ccf2]/40" />
            <span className="size-4 rounded-full border-2 border-white bg-[#56ccf2] shadow-[0_2px_6px_rgba(0,0,0,0.7)]" />
          </span>
        ) : (
          <svg width="24" height="24" viewBox="0 0 30 30" aria-hidden="true" style={{ transform: `rotate(${screen.angle}deg)` }} className="drop-shadow-[0_2px_4px_rgba(0,0,0,0.7)]">
            <path d="M15 2 L26 26 L15 20 L4 26 Z" fill="#56ccf2" stroke="#fff" strokeWidth="2" strokeLinejoin="round" />
          </svg>
        )}
        <div className="whitespace-nowrap rounded-full bg-black/65 px-2 py-0.5 text-[11.5px] font-semibold text-white shadow">
          {ping.from ?? 'Your ping'} <span className="font-normal text-white/75">· {formatDistance(meters, units)}</span>
        </div>
      </div>
    )
  })
}

/**
 * Hostiles close by: a count at the top of the screen; for each one off screen (or
 * behind), a red chevron at the screen edge pointing their way, with a glow on that
 * edge for the corner of your eye; on-screen ones get a marker where they are. The
 * closer, the bigger and stronger.
 */
export function DangerAlert() {
  const snapshot = useStore((s) => s.snapshot)
  const game = useStore((s) => s.game)
  const enabled = useGameSetting(game?.id, 'dangerAlert')
  const meters = useGameSetting(game?.id, 'dangerMeters')
  const units = useSettings((s) => s.ui.units)
  if (!enabled || !snapshot?.local || !game) return null
  const radius = meters * game.unitsPerMeter
  const threats = nearbyHostiles(snapshot, radius)
  if (threats.length === 0) return null
  const nearest = threats[0].distance / game.unitsPerMeter
  return (
    <>
      <div className="pointer-events-none absolute left-1/2 top-2 -translate-x-1/2 rounded-full bg-danger/85 px-3 py-0.5 text-[12.5px] font-semibold text-white shadow-lg">
        {threats.length === 1 ? '1 hostile' : `${threats.length} hostiles`} nearby · {formatDistance(nearest, units)}
      </div>
      {threats.map((t: Threat) => {
        const screen = screenPoint(snapshot, t.creature.position, DANGER_EDGE)
        if (!screen) return null
        // 0 at the edge of the alert distance, 1 right next to you.
        const close = 1 - t.distance / radius
        const strength = 0.5 + 0.5 * close
        if (screen.onScreen) {
          return (
            <span
              key={t.creature.id}
              className="pointer-events-none absolute size-3.5 -translate-x-1/2 -translate-y-1/2 rotate-45 border-2 border-white bg-danger shadow-[0_0_8px_rgba(239,68,68,0.8)]"
              style={{ left: screen.x, top: screen.y, opacity: strength }}
            />
          )
        }
        const size = 26 + 18 * close
        const glow = 140 + 120 * close
        return (
          <div key={t.creature.id}>
            <span
              className="pointer-events-none absolute rounded-full"
              style={{
                left: screen.x - glow / 2,
                top: screen.y - glow / 2,
                width: glow,
                height: glow,
                background: `radial-gradient(circle, rgba(239,68,68,${0.38 * strength}) 0%, rgba(239,68,68,0) 70%)`,
              }}
            />
            <svg
              width={size}
              height={size * 0.64}
              viewBox="0 0 44 28"
              aria-hidden="true"
              className="pointer-events-none absolute drop-shadow-[0_2px_4px_rgba(0,0,0,0.8)]"
              style={{ left: screen.x - size / 2, top: screen.y - (size * 0.64) / 2, opacity: strength, transform: `rotate(${screen.angle}deg)` }}
            >
              <path d="M4 24 L22 6 L40 24" fill="none" stroke="var(--color-danger)" strokeWidth="6" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          </div>
        )
      })}
    </>
  )
}
