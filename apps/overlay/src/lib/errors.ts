// Sends UI errors and events to the app's log, where they can be found after the fact.

import { backend } from './backend'

/** The same error again within this time is counted, not written: a bad frame repeats. */
const REPEAT_MS = 10_000
const MAX_TRACKED = 200

/** Automatic page reloads allowed in `RELOAD_WINDOW_MS`: past that, reloading isn't helping. */
const MAX_RELOADS = 3
const RELOAD_WINDOW_MS = 10 * 60_000
/** Per window: storage is shared, and one window's trouble isn't another's. */
const reloadsKey = () => `game-maps:reloads:${window.location.pathname}${window.location.search}`

/** Lets each message through once per `REPEAT_MS`, saying how many were held back. */
export function repeatFilter() {
  const seen = new Map<string, { at: number; held: number }>()
  return (message: string, now = Date.now()): string | null => {
    const last = seen.get(message)
    if (last && now - last.at < REPEAT_MS) {
      last.held++
      return null
    }
    if (seen.size >= MAX_TRACKED) seen.clear()
    seen.set(message, { at: now, held: 0 })
    return last?.held ? `${message} (and ${last.held} more like it)` : message
  }
}

const filter = repeatFilter()

/** Writes to the log; a failure to log is never itself an error. */
export function logUiError(message: string) {
  const line = filter(message)
  if (line) void backend.logUiError(line).catch(() => {})
}

/** Writes an event worth finding later to the log; never fails, like `logUiError`. */
export function logUiInfo(message: string) {
  void backend.logUiInfo(message).catch(() => {})
}

/** Logs errors nothing caught. Call once per window. */
export function startErrorLogging() {
  window.addEventListener('error', (e) => logUiError(e.error instanceof Error && e.error.stack ? e.error.stack : `${e.message} (${e.filename}:${e.lineno})`))
  window.addEventListener('unhandledrejection', (e) => logUiError(`unhandled rejection: ${e.reason instanceof Error && e.reason.stack ? e.reason.stack : String(e.reason)}`))
}

/** Recent reload times with one more, or null when there have been too many. */
export function allowReload(previous: number[], now: number): number[] | null {
  const recent = previous.filter((t) => now - t < RELOAD_WINDOW_MS)
  return recent.length >= MAX_RELOADS ? null : [...recent, now]
}

/** Reloads the page unless that keeps happening; false when it won't. */
export function reloadPage(): boolean {
  try {
    const times = allowReload(JSON.parse(localStorage.getItem(reloadsKey()) ?? '[]') as number[], Date.now())
    if (!times) return false
    localStorage.setItem(reloadsKey(), JSON.stringify(times))
  } catch {
    // Without storage there's no count to keep, and one reload still beats a dead page.
  }
  logUiError('the page keeps failing: reloading it')
  window.location.reload()
  return true
}
