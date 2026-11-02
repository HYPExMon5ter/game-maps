import type { Map as MlMap } from 'maplibre-gl'

/** The active full-size map, for imperative actions (fly to a pin, etc.). */
export const mapHandle: { current: MlMap | null } = { current: null }
