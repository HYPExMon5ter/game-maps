import { HUD_MARGIN, hudSize } from '../lib/hudLayout'
import { anchoredRect } from '../lib/layout'
import { useWindowSize } from '../lib/uiScale'
import { MapView } from '../map/MapView'
import { useStore } from '../state/store'
import { Compass } from './Compass'
import { SyncNotices } from './SyncRequests'
import { Waypoint } from './Waypoint'
import { formatEnvironment } from '../lib/environment'
import { DangerAlert, FriendMarkers, PingMarkers } from './OverGame'
import { useObscured, useOverGame } from '../lib/overGame'
import { useGameHud, useSettings } from '../settings/store'

/**
 * Click-through HUD shown while playing: mini-map and/or tracking compass. While
 * something is drawn over the game (waypoint, pings, danger alert), the window covers
 * the game's screen and the HUD sits where it was placed.
 *
 * The tree is the same either way, so the mini-map isn't rebuilt (and doesn't flash)
 * when the window grows or shrinks; the layout follows the window's actual size, which
 * changes a moment after the decision to cover the screen.
 */
export function Hud() {
  const hud = useGameHud(useStore((s) => s.game?.id))
  const over = useOverGame()
  // Kept mounted under the open map; what follows the game can rest meanwhile.
  const pinned = useStore((s) => s.mode === 'pinned')
  // Nothing to show at a menu; what's drawn over the game (a friend's request) still is.
  const minimap = hud.minimap && over.playing
  const tracking = useStore((s) => s.tracked !== null) && over.playing
  const win = useWindowSize()
  const size = hudSize(minimap, tracking, hud)
  const covering = win.width > size.width + 2 * HUD_MARGIN || win.height > size.height + 2 * HUD_MARGIN
  // At a fixed size either way: while the window changes size, the panel never stretches.
  const panel = covering ? anchoredRect(hud.anchor, size, win, HUD_MARGIN) : { left: 0, top: 0, ...size }
  const obscured = useObscured(panel, covering && hud.autoHide)
  return (
    <div className="hud relative h-full w-full">
      {(minimap || tracking) && (
        <div className={`absolute transition-opacity duration-200 ${obscured ? 'opacity-0' : ''}`} style={panel}>
          <HudPanel tracking={tracking} />
        </div>
      )}
      {/* After the panel: what's drawn over the game stays on top of the mini-map. */}
      {covering && pinned && (
        <>
          {over.waypoint && <Waypoint />}
          {over.friends && <FriendMarkers />}
          {over.pings && <PingMarkers />}
          {over.danger && <DangerAlert />}
          <SyncNotices />
        </>
      )}
    </div>
  )
}

function HudPanel({ tracking }: { tracking: boolean }) {
  const hud = useSettings((s) => s.ui.hud)
  const hasLocation = useStore((s) => !!s.snapshot?.local)
  // The text, not the object: each snapshot's is new, and that would redraw all of this.
  const environment = useStore((s) => (s.snapshot?.environment ? formatEnvironment(s.snapshot.environment) : null))

  return (
    <div className="flex h-full w-full flex-col gap-2" style={{ opacity: hud.opacity }}>
      {hud.minimap && (
        <div className="relative min-h-0 flex-1 overflow-hidden rounded-[22px] shadow-xl ring-2 ring-white/20">
          <MapView variant="mini" />
          {!hasLocation && (
            <div className="absolute inset-0 grid place-items-center bg-black/45 text-[12px] text-ink-dim">Waiting for location…</div>
          )}
          {hud.clock && environment && (
            <div className="absolute inset-x-0 bottom-0 truncate bg-black/55 px-2 py-0.5 text-center text-[11px] font-medium tabular-nums text-white">{environment}</div>
          )}
        </div>
      )}
      {tracking && <Compass compact interactive={false} />}
    </div>
  )
}
