// How long the map's toasts stay up.

/** Shortest time a toast shows. */
const MIN_MS = 2600
/** Errors say what went wrong and often what to do: they get read, not glanced at. */
const MIN_ERROR_MS = 5000
const MAX_MS = 10_000
/** Reading time per character, on top of a moment to notice the toast. */
const NOTICE_MS = 1000
const MS_PER_CHARACTER = 65

/** Time to show a toast for: longer for more text, and for errors. */
export function toastMs(text: string, tone: 'info' | 'error'): number {
  const reading = NOTICE_MS + text.length * MS_PER_CHARACTER
  return Math.min(MAX_MS, Math.max(tone === 'error' ? MIN_ERROR_MS : MIN_MS, reading))
}
