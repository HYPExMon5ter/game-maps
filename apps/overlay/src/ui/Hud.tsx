import { MapView } from '../map/MapView'
import { useStore } from '../state/store'
import { Compass } from './Compass'

/** Click-through HUD shown while playing: mini-map and/or tracking compass. */
export function Hud() {
  const hud = useStore((s) => s.settings.hud)
  const tracking = useStore((s) => s.tracked !== null)
  const hasLocation = useStore((s) => !!s.snapshot?.local)

  return (
    <div className="flex h-full w-full flex-col gap-2" style={{ opacity: hud.opacity }}>
      {hud.minimap && (
        <div className="relative min-h-0 flex-1 overflow-hidden rounded-[22px] shadow-xl ring-2 ring-white/20">
          <MapView variant="mini" />
          {!hasLocation && (
            <div className="absolute inset-0 grid place-items-center bg-black/45 text-[12px] text-ink-dim">Waiting for location…</div>
          )}
        </div>
      )}
      {tracking && <Compass compact interactive={false} />}
    </div>
  )
}

export const COMPASS_HEIGHT = 70
export const HUD_GAP = 8
