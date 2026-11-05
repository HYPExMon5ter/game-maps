// Path history: records where the player walks while the app runs (see `lib/trail`).

import { useEffect } from 'react'
import { create } from 'zustand'
import { extendTrail, type TrailPoint } from '../lib/trail'
import { useStore } from './store'

interface TrailState {
  /** The world the trail was walked in. */
  worldId: string | null
  points: TrailPoint[]
}

export const useTrailStore = create<TrailState>(() => ({ worldId: null, points: [] }))

export function clearTrail() {
  useTrailStore.setState({ points: [] })
}

/** Records the player's path, starting over in each world. */
export function useTrail() {
  useEffect(
    () =>
      useStore.subscribe((s, prev) => {
        const local = s.snapshot?.local
        if (s.snapshot === prev.snapshot || !local || !s.map || !s.game) return
        const trail = useTrailStore.getState()
        const points = trail.worldId === s.world.id ? trail.points : []
        const next = extendTrail(points, local, s.map.projection, s.game.unitsPerMeter)
        if (next !== trail.points) useTrailStore.setState({ worldId: s.world.id, points: next })
      }),
    [],
  )
}
