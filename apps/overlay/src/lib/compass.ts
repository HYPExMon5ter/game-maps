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

/** Imperial (feet and miles, the default) or metric (meters and kilometers). */
export type Units = 'imperial' | 'metric'

const FEET_PER_METER = 3.28084
const METERS_PER_MILE = 1609.344

/** A distance for display: feet up to a tenth of a mile, then miles (or m / km). */
export function formatDistance(meters: number, units: Units): string {
  if (units === 'metric') {
    if (meters < 1000) return `${Math.round(meters)} m`
    return `${(meters / 1000).toFixed(meters < 10_000 ? 2 : 1)} km`
  }
  const miles = meters / METERS_PER_MILE
  if (miles < 0.1) return `${Math.round(meters * FEET_PER_METER)} ft`
  return `${miles.toFixed(miles < 10 ? 2 : 1)} mi`
}

/** A height difference for display: whole feet or meters. */
export function formatHeight(meters: number, units: Units): string {
  return units === 'metric' ? `${Math.round(meters)} m` : `${Math.round(meters * FEET_PER_METER)} ft`
}
