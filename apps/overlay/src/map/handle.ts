import type { Map as MlMap } from 'maplibre-gl'
import type { Vec3 } from '../lib/protocol'

/** The active full-size map, for imperative actions (fly to a pin, etc.). */
export const mapHandle: { current: MlMap | null } = { current: null }

/** The underground 3D view while it's shown over the full map. */
export interface View3DHandle {
  /** Brings a world position into the middle of the view. */
  show(position: Vec3): void
  /** Zooms in (positive) or out by steps, as the map's zoom keys do. */
  zoom(steps: number): void
}

export const view3dHandle: { current: View3DHandle | null } = { current: null }
