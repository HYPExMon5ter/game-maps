// Which layers are drawn over the game (see `ui/OverGame`), deciding whether the HUD
// window covers the game's screen.

import { useGameSetting, useSettings } from '../settings/store'
import { useStore } from '../state/store'
import { useEffect, useState } from 'react'
import { nearbyHostiles } from './danger'
import type { CameraView, Vec3, WorldSnapshot } from './protocol'
import { useSyncStore } from './sync/sync'
import { projectToScreen } from './waypoint'

/** How far in from the screen edge danger chevrons and ping arrows sit (CSS pixels). */
export const DANGER_EDGE = 26
export const PING_EDGE = 56
/** How close to the mini-map a marker counts as in the way (CSS pixels). */
const OBSCURE_PAD = 36
/** How long the mini-map stays hidden after the last marker left it. */
const OBSCURE_HOLD_MS = 1500

export interface Rect {
  left: number
  top: number
  width: number
  height: number
}

/** Whether any point is on `rect` or within `pad` of it. */
export function pointsNear(points: { x: number; y: number }[], rect: Rect, pad: number): boolean {
  return points.some((p) => p.x >= rect.left - pad && p.x <= rect.left + rect.width + pad && p.y >= rect.top - pad && p.y <= rect.top + rect.height + pad)
}

/** A screen point for `p`, from the camera when there is one, else the player's facing. */
export function screenPoint(snapshot: WorldSnapshot, p: Vec3, margin: number) {
  const local = snapshot.local
  if (!local) return null
  const camera: CameraView | null =
    local.camera ??
    (local.forward
      ? // No camera from the agent: look along the player's facing, from about eye height.
        { position: { ...local.position, y: local.position.y + 1.6 }, forward: local.forward, up: { x: 0, y: 1, z: 0 }, fov_y: 60, aspect: window.innerWidth / window.innerHeight }
      : null)
  return camera ? projectToScreen(camera, p, window.innerWidth, window.innerHeight, margin) : null
}

/**
 * Whether a danger marker or ping is drawn on or next to `rect` (the mini-map). True at
 * once, and for a moment after the last one leaves, so it doesn't flicker in a fight.
 */
export function useObscured(rect: Rect, enabled: boolean): boolean {
  const gameId = useStore((s) => s.game?.id)
  const dangerOn = useGameSetting(gameId, 'dangerAlert')
  const dangerMeters = useGameSetting(gameId, 'dangerMeters')
  const pingsOn = useGameSetting(gameId, 'pingsInGame')
  const pings = useSyncStore((s) => s.pings)
  const near = useStore((s) => {
    const snapshot = s.snapshot
    if (!enabled || !snapshot?.local) return false
    const points = []
    if (dangerOn) {
      for (const t of nearbyHostiles(snapshot, dangerMeters * (s.game?.unitsPerMeter ?? 1))) points.push(screenPoint(snapshot, t.creature.position, DANGER_EDGE))
    }
    if (pingsOn) for (const p of pings) points.push(screenPoint(snapshot, p.position, PING_EDGE))
    return pointsNear(points.filter((p) => p !== null), rect, OBSCURE_PAD)
  })
  const [held, setHeld] = useState(false)
  useEffect(() => {
    const id = setTimeout(() => setHeld(near), near ? 0 : OBSCURE_HOLD_MS)
    return () => clearTimeout(id)
  }, [near])
  return near || held
}

/** What's drawn over the game right now. */
export interface OverGame {
  waypoint: boolean
  pings: boolean
  danger: boolean
  /**
   * Whether the HUD window should cover the game's screen. Set by what could be drawn,
   * not what is: moving and resizing the window as enemies come and go makes it flash.
   */
  cover: boolean
}

/** Which over-the-game layers have something to show (decides the HUD window's size). */
export function useOverGame(): OverGame {
  const gameId = useStore((s) => s.game?.id)
  const tracking = useStore((s) => s.tracked !== null)
  const waypointOn = useSettings((s) => s.ui.hud.waypoint)
  const pingsOn = useGameSetting(gameId, 'pingsInGame')
  const hasPings = useSyncStore((s) => s.pings.length > 0)
  const inSession = useSyncStore((s) => s.session !== null)
  const dangerOn = useGameSetting(gameId, 'dangerAlert')
  const dangerMeters = useGameSetting(gameId, 'dangerMeters')
  const danger = useStore((s) => dangerOn && nearbyHostiles(s.snapshot, dangerMeters * (s.game?.unitsPerMeter ?? 1), 1).length > 0)
  const waypoint = tracking && waypointOn
  return { waypoint, pings: hasPings && pingsOn, danger, cover: waypoint || dangerOn || (pingsOn && inSession) || hasPings }
}

