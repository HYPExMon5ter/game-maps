// HTML markers for the local player (heading arrow), other players and companions
// such as Kelvin (colored dot + name). Markers are moved in place on each snapshot; no React re-render involved.

import { Marker, type Map as MlMap } from 'maplibre-gl'
import { movers } from '../lib/movers'
import type { WorldSnapshot } from '../lib/protocol'
import { bearingOf, toLngLat, type Projection } from '../lib/projection'

function localElement(mini: boolean): HTMLElement {
  const el = document.createElement('div')
  el.className = 'player-local'
  const size = mini ? 30 : 34
  el.innerHTML = `
    <svg width="${size}" height="${size}" viewBox="0 0 34 34" aria-hidden="true">
      <circle cx="17" cy="17" r="15" fill="rgba(56,189,248,0.18)" />
      <path d="M17 4 L26 27 L17 21.5 L8 27 Z" fill="#38bdf8" stroke="#fff" stroke-width="2" stroke-linejoin="round" />
    </svg>`
  return el
}

function remoteElement(name: string, color: string, onClick?: () => void): HTMLElement {
  const el = document.createElement('div')
  el.className = 'player-remote'
  if (onClick) {
    el.classList.add('player-remote-clickable')
    el.addEventListener('click', onClick)
  }
  const dot = document.createElement('div')
  dot.className = 'player-remote-dot'
  dot.style.background = color
  const label = document.createElement('div')
  label.className = 'player-remote-label'
  label.textContent = name
  el.append(dot, label)
  return el
}

export class PlayerMarkers {
  #map: MlMap
  #projection: Projection
  #mini: boolean
  #local: Marker | null = null
  #remote = new Map<string, { marker: Marker; name: string; color: string }>()
  #onSelect?: (id: string) => void

  /** `onSelect`: called with a player's or companion's id when its marker is clicked. */
  constructor(map: MlMap, projection: Projection, mini: boolean, onSelect?: (id: string) => void) {
    this.#map = map
    this.#projection = projection
    this.#mini = mini
    this.#onSelect = onSelect
  }

  update(snapshot: WorldSnapshot | null) {
    const local = snapshot?.local ?? null
    if (local) {
      const ll = toLngLat(this.#projection, local.position)
      const heading = local.forward ? bearingOf(this.#projection, local.forward) : null
      this.#local ??= new Marker({ element: localElement(this.#mini), rotationAlignment: 'map' }).setLngLat(ll).addTo(this.#map)
      this.#local.setLngLat(ll)
      if (heading !== null) this.#local.setRotation(heading)
    } else if (this.#local) {
      this.#local.remove()
      this.#local = null
    }

    const seen = new Set<string>()
    for (const other of movers(snapshot)) {
      seen.add(other.id)
      const color = other.color
      const ll = toLngLat(this.#projection, other.position)
      let entry = this.#remote.get(other.id)
      if (entry && (entry.name !== other.name || entry.color !== color)) {
        entry.marker.remove()
        entry = undefined
      }
      if (!entry) {
        const id = other.id
        const onClick = this.#onSelect && (() => this.#onSelect?.(id))
        const marker = new Marker({ element: remoteElement(other.name, color, onClick) }).setLngLat(ll).addTo(this.#map)
        entry = { marker, name: other.name, color }
        this.#remote.set(other.id, entry)
      }
      entry.marker.setLngLat(ll)
    }
    for (const [id, entry] of this.#remote) {
      if (!seen.has(id)) {
        entry.marker.remove()
        this.#remote.delete(id)
      }
    }
  }

  remove() {
    this.#local?.remove()
    this.#local = null
    for (const e of this.#remote.values()) e.marker.remove()
    this.#remote.clear()
  }
}
