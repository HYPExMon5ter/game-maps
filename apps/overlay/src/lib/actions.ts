// User actions shared across components.

import { mapHandle } from '../map/handle'
import { selectAllPins, useStore } from '../state/store'
import { backend } from './backend'
import type { Pin } from './game'
import { LABEL_ZOOM } from '../map/style'
import { mapZoom, toLngLat, type LngLat, toWorld } from './projection'

function projection() {
  const map = useStore.getState().map
  if (!map) throw new Error('no map loaded')
  return map.projection
}

async function runTeleport(fn: () => Promise<void>, what: string) {
  const { notify } = useStore.getState()
  try {
    await fn()
    notify(`Teleported to ${what}`)
  } catch (e) {
    notify(`Teleport failed: ${String(e)}`, 'error')
  }
  void refreshBackSteps()
}

/** Updates how many teleports can be undone (shown on the Return button). */
export async function refreshBackSteps() {
  try {
    useStore.getState().setBackSteps(await backend.teleportBackSteps())
  } catch {
    // Backend unavailable (e.g. the app is shutting down): keep the last count.
  }
}

/** Returns to where the player was before their last teleport. */
export function teleportBack() {
  return runTeleport(() => backend.teleportBack(), 'where you were')
}

/** Height added above an item's exact position so the player lands on its floor, not in it. */
const PIN_CLEARANCE_METERS = 0.5

export function teleportToPin(pin: Pin) {
  const snap = pin.snapToGround === true
  const { game } = useStore.getState()
  const up = projection().up
  const position = snap ? pin.position : { ...pin.position, [up]: pin.position[up] + PIN_CLEARANCE_METERS * (game?.unitsPerMeter ?? 1) }
  return runTeleport(() => backend.teleport({ kind: 'position', position, snap_to_ground: snap }), pin.name)
}

export function teleportToMapPoint(ll: LngLat) {
  const pos = toWorld(projection(), ll, 0)
  return runTeleport(() => backend.teleport({ kind: 'position', position: pos, snap_to_ground: true }), 'map location')
}

/** Teleports next to a live entity (player, companion, golf cart, ...). */
export function teleportToPlayer(id: string, name: string) {
  return runTeleport(() => backend.teleport({ kind: 'player', id }), name)
}

export function flyToPin(pin: Pin) {
  const map = mapHandle.current
  if (!map) return
  const ll = toLngLat(projection(), pin.position)
  map.flyTo({ center: [ll.lng, ll.lat], zoom: Math.max(map.getZoom(), mapZoom(projection(), LABEL_ZOOM)), speed: 2 })
}

export function centerOnPlayer() {
  const map = mapHandle.current
  const local = useStore.getState().snapshot?.local
  if (!map || !local) return
  const ll = toLngLat(projection(), local.position)
  map.flyTo({ center: [ll.lng, ll.lat], zoom: Math.max(map.getZoom(), 4), speed: 2 })
}

export function findPin(id: string): Pin | undefined {
  return selectAllPins(useStore.getState()).find((p) => p.id === id)
}

export function newCustomPinAt(ll: LngLat): Pin {
  const pos = toWorld(projection(), ll, 0)
  return {
    id: `custom-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 7)}`,
    category: 'custom',
    name: 'New pin',
    position: pos,
    layer: useStore.getState().viewLayerOverride ?? (useStore.getState().snapshot?.local?.layer === 'caves' ? 'caves' : 'surface'),
    custom: true,
    snapToGround: true,
  }
}

export function customPinAtPlayer(): Pin | null {
  const local = useStore.getState().snapshot?.local
  if (!local) return null
  return {
    id: `custom-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 7)}`,
    category: 'custom',
    name: 'New pin',
    position: { ...local.position },
    layer: local.layer === 'caves' ? 'caves' : 'surface',
    custom: true,
  }
}
