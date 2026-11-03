// What the player is tracking: a pin or a live entity, resolved to a name and position.

import { findPin } from './actions'
import { findMover } from './movers'
import type { Vec3, WorldSnapshot } from './protocol'
import type { TrackTarget } from '../state/store'

export interface TrackedTarget {
  name: string
  /** Unknown while a live entity is out of range or gone. */
  position: Vec3 | undefined
}

export function resolveTracked(tracked: TrackTarget, snapshot: WorldSnapshot | null): TrackedTarget {
  if (tracked.kind === 'point') return { name: tracked.name, position: tracked.position }
  if (tracked.kind === 'pin') {
    const pin = findPin(tracked.id)
    return { name: pin?.name ?? '', position: pin?.position }
  }
  const entity = findMover(snapshot, tracked.id)
  return { name: entity?.name ?? 'Target', position: entity?.position }
}
