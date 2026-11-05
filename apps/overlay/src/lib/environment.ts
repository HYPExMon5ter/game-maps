// The time, day, season and weather as one line.

import type { Environment } from './protocol'

export function formatEnvironment(e: Environment): string {
  const time = `${String(e.hour).padStart(2, '0')}:${String(e.minute).padStart(2, '0')}`
  return [e.day !== null ? `Day ${e.day + 1}` : null, time, e.season, e.weather].filter(Boolean).join(' · ')
}
