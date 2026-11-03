import { HUD_MARGIN, hudSize } from '../lib/hudLayout'
import { MapView } from '../map/MapView'
import { useStore } from '../state/store'
import { Compass } from './Compass'
import { SyncRequestNotice } from './SyncRequests'
import { Waypoint } from './Waypoint'
import { useSettings } from '../settings/store'

/**
 * Click-through HUD shown while playing: mini-map and/or tracking compass. With a
 * waypoint, the window covers the game's screen and the HUD sits in its corner.
 */
export function Hud() {
  const hud = useSettings((s) => s.ui.hud)
  const tracking = useStore((s) => s.tracked !== null)
  if (!(tracking && hud.waypoint)) return <HudPanel />
  const size = hudSize(hud.minimap, tracking, hud)
  const [v, h] = hud.corner.split('_') as ['top' | 'bottom', 'left' | 'right']
  return (
    <div className="relative h-full w-full">
      <Waypoint />
      <div className="absolute" style={{ [v]: HUD_MARGIN, [h]: HUD_MARGIN, width: size.width, height: size.height }}>
        <HudPanel />
      </div>
    </div>
  )
}

function HudPanel() {
  const hud = useSettings((s) => s.ui.hud)
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
      <SyncRequestNotice />
    </div>
  )
}
