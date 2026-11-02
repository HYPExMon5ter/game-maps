import type { LocalPlayer, Vec3 } from './protocol'
import { bearingOf, groundDistance, type Projection } from './projection'

export interface CompassReading {
  /** Horizontal distance in meters. */
  distance: number
  /** Height difference (target − player) in meters. */
  rise: number
  /** Absolute bearing to the target, degrees clockwise from north. */
  bearing: number | null
  /** Bearing relative to where the camera faces (0 = straight ahead), or null without a heading. */
  relative: number | null
}

export function compassReading(p: Projection, player: LocalPlayer, target: Vec3, unitsPerMeter: number): CompassReading {
  const to: Vec3 = {
    x: target.x - player.position.x,
    y: target.y - player.position.y,
    z: target.z - player.position.z,
  }
  const bearing = bearingOf(p, to)
  const heading = player.forward ? bearingOf(p, player.forward) : null
  const relative = bearing === null || heading === null ? null : (((bearing - heading) % 360) + 540) % 360 - 180
  return {
    distance: groundDistance(p, player.position, target) / unitsPerMeter,
    rise: (target[p.up] - player.position[p.up]) / unitsPerMeter,
    bearing,
    relative,
  }
}

export function formatDistance(meters: number): string {
  if (meters < 1000) return `${Math.round(meters)} m`
  return `${(meters / 1000).toFixed(meters < 10_000 ? 2 : 1)} km`
}
