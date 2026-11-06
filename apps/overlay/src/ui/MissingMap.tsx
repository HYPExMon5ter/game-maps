import { Download, Loader2 } from 'lucide-react'
import { useEffect, useState } from 'react'
import { backend, events, type PackStatus } from '../lib/backend'
import { logUiError } from '../lib/errors'
import { packView } from '../launcher/view'
import { useStore } from '../state/store'
import { Button } from './controls'

/** Over a generated world with no map yet: it's made from the game the first time. */
function MissingWorldMap() {
  const ready = useStore((s) => s.worldMapReady)
  const name = useStore((s) => s.game?.name)
  const loaded = useStore((s) => !!s.snapshot?.world && s.snapshot.world.id === s.world.id)
  if (ready) return null
  return (
    <div className="pointer-events-none absolute inset-0 z-10 grid place-items-center">
      <div className="glass pointer-events-auto w-[320px] rounded-2xl p-4 text-center">
        {loaded ? (
          <>
            <p className="font-display text-[15px] font-semibold">Drawing this world's map</p>
            <Loader2 className="mx-auto mt-2 size-4 animate-spin text-ink-faint" />
          </>
        ) : (
          <>
            <p className="font-display text-[15px] font-semibold">No map of this world yet</p>
            <p className="mt-1 text-[12.5px] text-ink-dim">Every {name} world is different. Load this one with Game Maps connected, and its map is drawn from the game.</p>
          </>
        )}
      </div>
    </div>
  )
}

/** Over a map that isn't there yet, which would otherwise just be empty. */
export function MissingMap() {
  const worldMaps = useStore((s) => !!s.game?.worldMaps)
  return worldMaps ? <MissingWorldMap /> : <MissingPack />
}

function MissingPack() {
  const game = useStore((s) => s.game)
  const [packs, setPacks] = useState<PackStatus[]>([])
  useEffect(() => {
    const unlisten = events.mapPacks(setPacks)
    void backend.mapPacks().then(setPacks)
    return () => void unlisten.then((f) => f())
  }, [])
  const pack = packs.find((p) => p.game_id === game?.id)
  if (!game || !pack || pack.installed) return null
  const view = packView(pack)
  const download = () => void backend.installMapPack(game.id).catch((e: unknown) => logUiError(`could not download the map: ${String(e)}`))
  return (
    <div className="pointer-events-none absolute inset-0 z-10 grid place-items-center">
      <div className="glass pointer-events-auto w-[320px] rounded-2xl p-4 text-center">
        <p className="font-display text-[15px] font-semibold">The {game.name} map isn't downloaded yet</p>
        <p className={`mt-1 text-[12.5px] ${view.tone === 'error' ? 'text-danger' : 'text-ink-dim'}`}>{view.text}</p>
        {view.progress !== null && (
          <div className="mt-2.5 h-1.5 overflow-hidden rounded-full bg-black/40">
            <div className="h-full rounded-full bg-accent transition-[width]" style={{ width: `${view.progress * 100}%` }} />
          </div>
        )}
        {view.progress !== null && <Loader2 className="mx-auto mt-2 size-4 animate-spin text-ink-faint" />}
        {view.action && (
          <Button tone="primary" className="mt-2.5" onClick={download}>
            <Download className="size-4" /> {view.action}
          </Button>
        )}
      </div>
    </div>
  )
}
