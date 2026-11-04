import { useEffect, useState } from 'react'
import { HUD_MARGIN, hudSize } from '../lib/hudLayout'
import { MapView } from '../map/MapView'
import { useStore } from '../state/store'
import { Compass } from './Compass'
import { SyncRequestNotice } from './SyncRequests'
import { Waypoint } from './Waypoint'
import { DangerAlert, PingMarkers } from './OverGame'
import { useObscured, useOverGame } from '../lib/overGame'
import { useSettings } from '../settings/store'

/** The HUD window's size, kept current as the backend resizes it. */
function useWindowSize() {
  const [size, setSize] = useState(() => ({ width: window.innerWidth, height: window.innerHeight }))
  useEffect(() => {
    const onResize = () => setSize({ width: window.innerWidth, height: window.innerHeight })
    window.addEventListener('resize', onResize)
    return () => window.removeEventListener('resize', onResize)
  }, [])
  return size
}

/**
 * Click-through HUD shown while playing: mini-map and/or tracking compass. While
 * something is drawn over the game (waypoint, pings, danger alert), the window covers
 * the game's screen and the HUD sits in its corner.
 *
 * The tree is the same either way, so the mini-map isn't rebuilt (and doesn't flash)
 * when the window grows or shrinks; the layout follows the window's actual size, which
 * changes a moment after the decision to cover the screen.
 */
export function Hud() {
  const hud = useSettings((s) => s.ui.hud)
  const tracking = useStore((s) => s.tracked !== null)
  const over = useOverGame()
  const win = useWindowSize()
  const size = hudSize(hud.minimap, tracking, hud)
  const covering = win.width > size.width + 2 * HUD_MARGIN || win.height > size.height + 2 * HUD_MARGIN
  const [v, h] = hud.corner.split('_') as ['top' | 'bottom', 'left' | 'right']
  // Anchored to its corner at a fixed size either way: while the window changes size,
  // the panel never stretches, it at most shifts by the margin for a frame.
  const margin = covering ? HUD_MARGIN : 0
  const panel = { [v]: margin, [h]: margin, width: size.width, height: size.height }
  const obscured = useObscured(
    {
      left: h === 'left' ? margin : win.width - margin - size.width,
      top: v === 'top' ? margin : win.height - margin - size.height,
      width: size.width,
      height: size.height,
    },
    covering && hud.autoHide,
  )
  return (
    <div className="relative h-full w-full">
      {(hud.minimap || tracking) && (
        <div className={`absolute transition-opacity duration-200 ${obscured ? 'opacity-0' : ''}`} style={panel}>
          <HudPanel />
        </div>
      )}
      {/* After the panel: what's drawn over the game stays on top of the mini-map. */}
      {covering && (
        <>
          {over.waypoint && <Waypoint />}
          <PingMarkers />
          <DangerAlert />
        </>
      )}
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
