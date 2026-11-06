// The time, day, season and weather as one line.

import type { Environment } from './protocol'

/** How far to turn an arrow pointing up on a map turned to `bearing`, for wind blowing to `degrees`. */
export function windRotation(degrees: number, bearing: number): number {
  return (((degrees - bearing) % 360) + 360) % 360
}

export function formatEnvironment(e: Environment): string {
  // Some games only report the wind.
  if (e.day === null && e.hour === 0 && e.minute === 0 && !e.season && !e.weather) return ''
  const time = `${String(e.hour).padStart(2, '0')}:${String(e.minute).padStart(2, '0')}`
  return [e.day !== null ? `Day ${e.day + 1}` : null, time, e.season, e.weather].filter(Boolean).join(' · ')
}
