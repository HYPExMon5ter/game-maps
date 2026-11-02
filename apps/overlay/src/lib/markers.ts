// Live GPS markers that add something beyond the static pins: placed locators,
// vehicles that were moved, and anything else the in-game map shows that the pin data
// doesn't already cover.

import type { Pin } from './game'
import type { Marker } from './protocol'

/** A live marker this close to a static pin with the same icon is that pin. */
const SAME_PLACE_METERS = 6

export class LiveMarkerFilter {
  #byIcon = new Map<string, Pin[]>()
  #unitsPerMeter: number

  constructor(pins: Pin[], unitsPerMeter: number) {
    this.#unitsPerMeter = unitsPerMeter
    for (const pin of pins) {
      if (!pin.icon?.startsWith('game:')) continue
      const icon = pin.icon.slice(5)
      const list = this.#byIcon.get(icon) ?? []
      list.push(pin)
      this.#byIcon.set(icon, list)
    }
  }

  /** Markers to draw as live map icons (companions are drawn with the players). */
  visible(markers: Marker[]): Marker[] {
    const limit = SAME_PLACE_METERS * this.#unitsPerMeter
    return markers.filter((m) => {
      if (m.kind === 'companion' || !m.icon) return false
      const same = this.#byIcon.get(m.icon) ?? []
      return !same.some((p) => Math.hypot(p.position.x - m.position.x, p.position.y - m.position.y, p.position.z - m.position.z) < limit)
    })
  }
}

/** Display name for a GPS icon, e.g. `GolfCartIcon` -> `Golf Cart`. */
export function markerName(m: Marker): string {
  if (m.label) return m.label
  const base = (m.icon ?? 'GPS marker').replace(/(Icon|Logo)$/, '')
  return base.replace(/([a-z])([A-Z])/g, '$1 $2')
}
