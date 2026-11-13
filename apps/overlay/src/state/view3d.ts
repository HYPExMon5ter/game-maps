// The 3D map view: whether it's on (a setting, see `Settings.map3d`), which game data it
// has, and the underground view's own controls. A game's 3D data is an optional part of
// its map, downloaded only once the player asks for it (see `download3d`).

import { useEffect, useState } from 'react'
import { create } from 'zustand'
import { backend, events, type PackStatus } from '../lib/backend'
import { load3d, type Map3D } from '../lib/map3d'
import { useSettings } from '../settings/store'
import { selectViewLayer, useStore } from './store'

/** How much of the underground is cut away from above. */
export type Clip =
  /** Just above the player while they're underground: their level and below. */
  | { mode: 'auto' }
  /** Nothing cut away. */
  | { mode: 'off' }
  /** Everything above this world height. */
  | { mode: 'height'; height: number }

interface View3DState {
  clip: Clip
  /** The translucent ground surface over the caves. */
  ground: boolean
  /** A cave to bring into view, picked from the list (bumped to repeat a pick). */
  focus: { id: string; seq: number } | null
  /** A camera angle to turn to (bumped to repeat it). */
  tilt: { view: 'top' | 'angled'; seq: number } | null
  /** The cave or bunker in the middle of the view. */
  current: string | null
  /** Offering to download the 3D map (the player picked 3D before having it). */
  offer: boolean
  /** Bumped when new 3D data was installed, to load it again. */
  dataVersion: number
  setCurrent(id: string | null): void
  setClip(clip: Clip): void
  setGround(ground: boolean): void
  focusMesh(id: string): void
  tiltTo(view: 'top' | 'angled'): void
  setOffer(offer: boolean): void
  reloadData(): void
}

export const useView3D = create<View3DState>((set, get) => ({
  clip: { mode: 'auto' },
  ground: false,
  focus: null,
  tilt: null,
  current: null,
  offer: false,
  dataVersion: 0,
  setOffer: (offer) => set({ offer }),
  reloadData: () => set({ dataVersion: get().dataVersion + 1 }),
  setCurrent: (current) => set({ current }),
  setClip: (clip) => set({ clip }),
  setGround: (ground) => set({ ground }),
  focusMesh: (id) => set({ focus: { id, seq: (get().focus?.seq ?? 0) + 1 } }),
  tiltTo: (view) => set({ tilt: { view, seq: (get().tilt?.seq ?? 0) + 1 } }),
}))

/** Tilt the 3D surface view starts at (degrees from looking straight down). */
export const START_PITCH = 55

/** Head room left above the player's feet by the automatic cut (meters). */
export const AUTO_CLIP_ABOVE = 3.5

/** The world height the underground view is cut at, or null for none. */
export function clipHeight(clip: Clip, playerUp: number | null, unitsPerMeter: number): number | null {
  if (clip.mode === 'height') return clip.height
  if (clip.mode === 'auto' && playerUp !== null) return playerUp + AUTO_CLIP_ABOVE * unitsPerMeter
  return null
}

/** The current map's 3D data, once loaded (null: none, or not loaded yet). */
export function useMap3D(): Map3D | null {
  const gameId = useStore((s) => s.game?.id)
  const mapId = useStore((s) => s.map?.id)
  const version = useView3D((s) => s.dataVersion)
  const [data, setData] = useState<{ key: string; map: Map3D | null } | null>(null)
  const key = `${gameId}/${mapId}/${version}`
  useEffect(() => {
    if (!gameId || !mapId) return
    let cancelled = false
    void load3d(gameId, mapId).then((map) => {
      if (!cancelled) setData({ key, map })
    })
    return () => {
      cancelled = true
    }
  }, [gameId, mapId, key])
  return data?.key === key ? data.map : null
}

/** What the full map shows in 3D right now: the tilted surface, the caves, or nothing. */
export function use3DMode(): 'surface' | 'caves' | null {
  const on = useSettings((s) => s.ui.map3d)
  const layer = useStore(selectViewLayer)
  const data = useMap3D()
  if (!on || !data) return null
  if (layer === 'caves') return data.meshes.length ? 'caves' : null
  return layer === 'surface' && data.terrain ? 'surface' : null
}

/** The optional part of a map pack that holds its 3D data. */
export const EXTRA_3D = '3d'

/** Where the current game's 3D data is: here, ready to download, on its way, or none published. */
export type Download3D =
  | { state: 'none' }
  | { state: 'installed' }
  | { state: 'available'; size: number; error: string | null }
  | { state: 'downloading'; progress: number; total: number }

export function download3d(pack: PackStatus | undefined): Download3D {
  const local = pack?.extras?.[EXTRA_3D]
  if (local?.downloading) {
    const { received, total } = local.downloading
    return { state: 'downloading', progress: total > 0 ? Math.min(1, received / total) : 0, total }
  }
  if (local?.installed) return { state: 'installed' }
  const remote = pack?.available?.extras?.[EXTRA_3D]
  return remote ? { state: 'available', size: remote.size, error: local?.error ?? null } : { state: 'none' }
}

/** The current game's 3D data download (see `download3d`), kept up to date. */
export function use3DDownload(): Download3D {
  const gameId = useStore((s) => s.game?.id)
  const [packs, setPacks] = useState<PackStatus[]>([])
  useEffect(() => {
    const unlisten = events.mapPacks(setPacks)
    void backend.mapPacks().then(setPacks)
    return () => void unlisten.then((f) => f())
  }, [])
  return download3d(packs.find((p) => p.game_id === gameId))
}
