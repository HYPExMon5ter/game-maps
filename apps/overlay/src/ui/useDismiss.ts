import { useEffect, type RefObject } from 'react'

/** Closes an open menu on a click outside `root` or on Esc. */
export function useDismiss(open: boolean, root: RefObject<HTMLElement | null>, close: () => void) {
  useEffect(() => {
    if (!open) return
    const onPointer = (e: PointerEvent) => {
      if (!root.current?.contains(e.target as Node)) close()
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      // Closes this menu, not the map.
      e.stopPropagation()
      close()
    }
    document.addEventListener('pointerdown', onPointer)
    window.addEventListener('keydown', onKey, { capture: true })
    return () => {
      document.removeEventListener('pointerdown', onPointer)
      window.removeEventListener('keydown', onKey, { capture: true })
    }
  }, [open, root, close])
}
