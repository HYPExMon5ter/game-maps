// Sends UI errors to the app's log, where they can be found after the fact.

import { backend } from './backend'

/** Writes to the log; a failure to log is never itself an error. */
export function logUiError(message: string) {
  void backend.logUiError(message).catch(() => {})
}

/** Logs errors nothing caught. Call once per window. */
export function startErrorLogging() {
  window.addEventListener('error', (e) => logUiError(`${e.message} (${e.filename}:${e.lineno})`))
  window.addEventListener('unhandledrejection', (e) => logUiError(`unhandled rejection: ${String(e.reason)}`))
}
