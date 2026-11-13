// User actions shared across components.

import { mapHandle, view3dHandle } from '../map/handle'
import { isCategoryVisible, selectAllPins, useStore } from '../state/store'
import { isPinVisible } from './filters'
import { backend } from './backend'
import { load3d } from './map3d'
import { terrainHeight } from './terrain'
import type { Pin } from './game'
import { LABEL_ZOOM } from '../map/style'
import { nearestUnfound } from './nearest'
import { mapZoom, toLngLat, type LngLat, toWorld } from './projection'
import type { LocalPlayer, Vec3, WorldLayer } from './protocol'

export const DEATH_PIN = 'custom-death'

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
  return runTeleport(() => backend.teleport({ kind: 'position', position, snap_to_ground: snap, layer: pin.layer }), pin.name)
}

export function teleportToMapPoint(ll: LngLat) {
  const pos = toWorld(projection(), ll, 0)
  return runTeleport(() => backend.teleport({ kind: 'position', position: pos, snap_to_ground: true }), 'map location')
}

/** Teleports to an exact spot, height included (picked in the 3D view). */
export function teleportToWorld(position: Vec3, layer: WorldLayer) {
  const { game } = useStore.getState()
  const up = projection().up
  const lifted = { ...position, [up]: position[up] + PIN_CLEARANCE_METERS * (game?.unitsPerMeter ?? 1) }
  return runTeleport(() => backend.teleport({ kind: 'position', position: lifted, snap_to_ground: false, layer }), 'that spot')
}

/**
 * Tracks a spot picked on the map, so the route and compass lead there. `world` is the
 * exact spot when the view knows it (3D); a point on the 2D map goes on the ground,
 * or at the player's height underground.
 */
export async function navigateTo(ll: LngLat, world: Vec3 | undefined, layer: WorldLayer) {
  const s = useStore.getState()
  const p = projection()
  let position = world
  if (!position) {
    position = toWorld(p, ll, 0)
    const local = s.snapshot?.local
    const terrain = s.game && s.map ? (await load3d(s.game.id, s.map.id))?.terrain : undefined
    const ground = layer === 'surface' && terrain && s.game ? await terrainHeight(s.game.id, terrain, p, position) : null
    position[p.up] = ground ?? (local ? local.position[p.up] : 0)
  }
  useStore.getState().setTracked({ kind: 'point', name: 'Marked spot', position, layer })
}

/** Teleports next to a live entity (player, companion, golf cart, ...). */
export function teleportToPlayer(id: string, name: string) {
  return runTeleport(() => backend.teleport({ kind: 'player', id }), name)
}

export function flyToPin(pin: Pin) {
  if (view3dHandle.current) return view3dHandle.current.show(pin.position)
  const map = mapHandle.current
  if (!map) return
  const ll = toLngLat(projection(), pin.position)
  map.flyTo({ center: [ll.lng, ll.lat], zoom: Math.max(map.getZoom(), mapZoom(projection(), LABEL_ZOOM)), speed: 2 })
}

export function centerOnPlayer() {
  const local = useStore.getState().snapshot?.local
  if (view3dHandle.current && local) return view3dHandle.current.show(local.position)
  const map = mapHandle.current
  if (!map || !local) return
  const ll = toLngLat(projection(), local.position)
  map.flyTo({ center: [ll.lng, ll.lat], zoom: Math.max(map.getZoom(), 4), speed: 2 })
}

export function findPin(id: string): Pin | undefined {
  return selectAllPins(useStore.getState()).find((p) => p.id === id)
}

/** A new custom pin at a map point, or at an exact spot when the view knows its height. */
export function newCustomPinAt(ll: LngLat, world?: Vec3): Pin {
  const pos = world ?? toWorld(projection(), ll, 0)
  return {
    id: `custom-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 7)}`,
    category: 'custom',
    name: 'New pin',
    position: pos,
    layer: useStore.getState().viewLayerOverride ?? (useStore.getState().snapshot?.local?.layer === 'caves' ? 'caves' : 'surface'),
    custom: true,
    snapToGround: !world,
  }
}

/** Drops a pin where the player stands (the quick-mark key). */
export function quickMark(): Pin | null {
  const pin = customPinAtPlayer()
  if (!pin) return null
  const { user, upsertCustomPin } = useStore.getState()
  const marked = { ...pin, name: `Mark ${user.customPins.filter((p) => p.name.startsWith('Mark ')).length + 1}` }
  upsertCustomPin(marked)
  return marked
}

/** The pin dropped where the player died, replacing the last one. */
export function markDeath(at: LocalPlayer) {
  const { upsertCustomPin } = useStore.getState()
  upsertCustomPin({
    id: DEATH_PIN,
    category: 'custom',
    name: 'Where you died',
    position: { ...at.position },
    layer: at.layer === 'caves' ? 'caves' : 'surface',
    color: '#eb5757',
    icon: 'builtin:skull',
    custom: true,
  })
}

/** How far the bag has to move to count: it settles a little after landing. */
const BAG_MOVE_UNITS = 1

/** Whether the bag is somewhere new. */
export function bagMoved(before: Vec3 | undefined, now: Vec3): boolean {
  return !before || Math.hypot(now.x - before.x, now.y - before.y, now.z - before.z) > BAG_MOVE_UNITS
}

/** Moves the death pin to the player's bag, which the game drops somewhere of its own. */
export function markBag(at: Vec3, layer: WorldLayer = 'surface') {
  useStore.getState().upsertCustomPin({
    id: DEATH_PIN,
    category: 'custom',
    name: useStore.getState().game?.bagName ?? 'Your bag',
    position: { ...at },
    layer: layer === 'caves' ? 'caves' : 'surface',
    color: '#eb5757',
    icon: 'builtin:backpack',
    custom: true,
  })
}

/** Removes the death pin, once there's nothing to go back for. */
export function clearDeathPin() {
  useStore.getState().deleteCustomPin(DEATH_PIN)
}

/** Tracks the nearest thing not found yet among the categories on the map. */
export function trackNearestUnfound(hidden?: (pin: Pin) => boolean): Pin | null {
  const s = useStore.getState()
  const local = s.snapshot?.local
  if (!local || !s.game || !s.map) return null
  const shown = s.game.categories.filter((c) => isCategoryVisible(c, s.user))
  const collectible = shown.filter((c) => c.autoFound)
  const categories = new Map(s.game.categories.map((c) => [c.id, c]))
  const pins = s.gamePins.filter((p) => isPinVisible(categories.get(p.category), p, s.user))
  const pin = nearestUnfound(pins, collectible.length ? collectible : shown, s.user.found, local, s.map.projection, hidden)
  if (pin) s.setTracked({ kind: 'pin', id: pin.id })
  return pin
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
