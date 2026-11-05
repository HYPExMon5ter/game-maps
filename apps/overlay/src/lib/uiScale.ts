// Interface scale: panels, text and buttons grow with the screen so the overlay reads
// the same at 720p, 1440p ultrawide and 4K. The map itself is never scaled.

import { useEffect, useState } from 'react'
import { useSettings } from '../settings/store'

/** Viewport height (CSS px) the interface is designed for. */
const BASE_HEIGHT = 1080
export const MIN_UI_SCALE = 0.75
export const MAX_UI_SCALE = 1.75

/**
 * Automatic scale for a viewport height. Grows with the square root of the height so
 * large screens get larger controls without them taking over the map.
 */
export function autoUiScale(viewportHeight: number): number {
  const s = Math.sqrt(Math.max(viewportHeight, 1) / BASE_HEIGHT)
  return Math.round(Math.min(MAX_UI_SCALE, Math.max(MIN_UI_SCALE, s)) * 100) / 100
}

/** This window's size, kept current as it changes. */
export function useWindowSize() {
  const [size, setSize] = useState(() => ({ width: window.innerWidth, height: window.innerHeight }))
  useEffect(() => {
    const onResize = () => setSize({ width: window.innerWidth, height: window.innerHeight })
    window.addEventListener('resize', onResize)
    return () => window.removeEventListener('resize', onResize)
  }, [])
  return size
}

/** Current interface scale: the user's choice, or automatic for this screen. */
export function useUiScale(): number {
  const manual = useSettings((s) => s.ui.uiScale)
  const { height } = useWindowSize()
  return manual ?? autoUiScale(height)
}
