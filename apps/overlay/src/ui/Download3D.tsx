// Over the map once the player picks 3D without having the 3D data yet: what it is and
// how big, then the download's progress. The 3D view turns on by itself when it's done.

import { Box, Download } from 'lucide-react'
import { backend } from '../lib/backend'
import { logUiError } from '../lib/errors'
import { formatSize } from '../launcher/view'
import { useSettings } from '../settings/store'
import { useStore } from '../state/store'
import { EXTRA_3D, use3DDownload, useView3D } from '../state/view3d'
import { Button } from './controls'

export function Download3D() {
  const gameId = useStore((s) => s.game?.id)
  const offer = useView3D((s) => s.offer)
  const setOffer = useView3D((s) => s.setOffer)
  const on = useSettings((s) => s.ui.map3d)
  const setUi = useSettings((s) => s.setUi)
  const download = use3DDownload()
  if (!gameId || !(offer || on) || (download.state !== 'available' && download.state !== 'downloading')) return null

  const start = () => {
    setUi({ map3d: true })
    setOffer(false)
    void backend.installMapExtra(gameId, EXTRA_3D).catch((e: unknown) => logUiError(`could not download the 3D map: ${String(e)}`))
  }
  const use2d = () => {
    setUi({ map3d: false })
    setOffer(false)
  }

  return (
    <div className="pointer-events-none absolute inset-0 z-10 grid place-items-center">
      <div className="glass pointer-events-auto w-[340px] rounded-2xl p-4 text-center" role="dialog" aria-label="Download the 3D map">
        <Box className="mx-auto size-5 text-accent" />
        {download.state === 'downloading' ? (
          <>
            <p className="mt-1.5 font-display text-[15px] font-semibold">Downloading the 3D map</p>
            <div className="mt-3 h-1.5 overflow-hidden rounded-full bg-black/40">
              <div className="h-full rounded-full bg-accent transition-[width]" style={{ width: `${download.progress * 100}%` }} />
            </div>
            <p className="mt-1.5 text-[12.5px] tabular-nums text-ink-dim">
              {Math.round(download.progress * 100)}% of {formatSize(download.total)}. It opens as soon as it's here.
            </p>
            <Button className="mt-3" onClick={use2d}>
              Use 2D for now
            </Button>
          </>
        ) : download.error ? (
          <>
            <p className="mt-1.5 font-display text-[15px] font-semibold">The 3D map didn't download</p>
            <p className="mt-1 text-[12.5px] text-danger">{download.error}</p>
            <div className="mt-3 flex justify-center gap-2">
              <Button onClick={use2d}>Use 2D</Button>
              <Button tone="primary" onClick={start}>
                <Download className="size-4" /> Try again
              </Button>
            </div>
          </>
        ) : (
          <>
            <p className="mt-1.5 font-display text-[15px] font-semibold">Download the 3D map?</p>
            <p className="mt-1 text-[12.5px] text-ink-dim">
              It tilts the island to show its hills, and shows every cave and bunker as a 3D model you can turn around. It's a
              separate {formatSize(download.size)} download, so the map stays small unless you want it.
            </p>
            <div className="mt-3 flex justify-center gap-2">
              <Button onClick={use2d}>Not now</Button>
              <Button tone="primary" onClick={start}>
                <Download className="size-4" /> Download {formatSize(download.size)}
              </Button>
            </div>
          </>
        )}
      </div>
    </div>
  )
}
