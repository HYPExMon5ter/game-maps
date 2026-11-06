// A generated world's biomes: what's under the cursor, and the way to the nearest of each.

import { Compass } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { logUiError } from '../lib/errors'
import { exploredAt } from '../lib/fog'
import { biomeAt, groundPoint, loadBiomes, nearestBiome, type BiomeGrid } from '../lib/biomes'
import { toLngLat, toWorld } from '../lib/projection'
import { mapHandle } from '../map/handle'
import { useSettings } from '../settings/store'
import { useFogStore } from '../state/fog'
import { useStore } from '../state/store'

export function BiomeTools() {
  const game = useStore((s) => s.game)
  const worldId = useStore((s) => s.world.id)
  const version = useStore((s) => s.worldMapVersion)
  const map = useStore((s) => s.map)
  const [grid, setGrid] = useState<BiomeGrid | null>(null)
  const [under, setUnder] = useState<number | null>(null)
  const [missing, setMissing] = useState<string | null>(null)
  const names = useRef(new Map<number, string>())
  names.current = new Map(game?.biomes?.map((b) => [b.id, b.name]))

  useEffect(() => {
    if (!game?.biomes || !map) return
    let current = true
    loadBiomes(game.id, worldId, map.projection)
      .then((g) => current && setGrid(g))
      .catch((e: unknown) => logUiError(`could not load the world's biomes: ${String(e)}`))
    return () => {
      current = false
    }
  }, [game, worldId, version, map])

  // Spoils nothing: what's under clouds isn't told while the map is hidden.
  const hiding = useSettings((s) => s.ui.fogOfWar && s.ui.fogHideMap)
  const revealed = (east: number, north: number) => {
    const fog = useFogStore.getState().fog
    return !hiding || !fog || exploredAt(fog, east, north)
  }

  useEffect(() => {
    const ml = mapHandle.current
    if (!ml || !grid || !map) return
    const p = map.projection
    const move = (e: { lngLat: { lng: number; lat: number } }) => {
      const w = toWorld(p, e.lngLat)
      setUnder(revealed(w[p.east], w[p.north]) ? biomeAt(grid, w[p.east], w[p.north]) : null)
    }
    ml.on('mousemove', move)
    return () => void ml.off('mousemove', move)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [grid, map, hiding])

  if (!game?.biomes || !grid || !map) return null

  const find = (id: number, name: string) => {
    const s = useStore.getState()
    const p = map.projection
    const ml = mapHandle.current
    const here = s.snapshot?.local?.position
    const center = ml ? toWorld(p, ml.getCenter()) : null
    const from = here ?? center
    if (!from) return
    const spot = nearestBiome(grid, id, from[p.east], from[p.north], revealed)
    setMissing(spot ? null : name)
    if (!spot) return
    const position = groundPoint(p, spot.east, spot.north)
    s.setTracked({ kind: 'point', name, position })
    const ll = toLngLat(p, position)
    ml?.flyTo({ center: [ll.lng, ll.lat] })
  }

  return (
    <div className="mt-3 px-2">
      <div className="pb-1 text-[11px] font-semibold uppercase tracking-wider text-ink-faint">
        Nearest biome{under !== null && <span className="ml-1 normal-case tracking-normal text-ink-dim">· {names.current.get(under)} under the cursor</span>}
      </div>
      <div className="flex flex-wrap gap-1">
        {game.biomes.map((b) => (
          <button key={b.id} className="inline-flex items-center gap-1 rounded-md bg-panel-raised px-1.5 py-0.5 text-[11.5px] hover:bg-white/10" onClick={() => find(b.id, b.name)}>
            <Compass className="size-3 text-ink-faint" /> {b.name}
          </button>
        ))}
      </div>
      {missing && <p className="mt-1 text-[11.5px] text-ink-faint">No {missing} found{hiding ? " in what you've explored" : ''}.</p>}
    </div>
  )
}
