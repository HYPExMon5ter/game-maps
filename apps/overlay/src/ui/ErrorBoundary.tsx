// Keeps a UI error from taking a whole window down: logs it, shows what happened, and
// tries again on its own (the overlay may be hidden, with nobody to click). When that
// doesn't help it reloads the page, a limited number of times.

import { Component, type ErrorInfo, type ReactNode } from 'react'
import { backend } from '../lib/backend'
import { logUiError, reloadPage } from '../lib/errors'
import { saveProblemReport } from '../lib/report'

const RETRY_MS = 3000
const MAX_RETRIES = 3
/** Errors further apart than this are separate problems, not one that keeps coming back. */
const HEALTHY_MS = 60_000

export class ErrorBoundary extends Component<{ children: ReactNode }, { error: Error | null }> {
  state = { error: null as Error | null }
  #retries = 0
  #lastError = 0
  #timer: ReturnType<typeof setTimeout> | undefined

  static getDerivedStateFromError(error: Error) {
    return { error }
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    logUiError(`${error.stack ?? error.message}${info.componentStack ?? ''}`)
    const now = Date.now()
    if (now - this.#lastError > HEALTHY_MS) this.#retries = 0
    this.#lastError = now
    this.#timer = setTimeout(() => {
      if (this.#retries >= MAX_RETRIES) {
        // Out of reloads too: the message stays up, with a way to report it.
        reloadPage()
        return
      }
      this.#retries++
      this.setState({ error: null })
    }, RETRY_MS)
  }

  componentWillUnmount() {
    clearTimeout(this.#timer)
  }

  render() {
    if (!this.state.error) return this.props.children
    return (
      <div className="grid h-full place-items-center">
        <div className="glass max-w-md rounded-2xl p-5 text-[13px]">
          <h2 className="font-display mb-2 text-[16px] font-semibold">Something went wrong</h2>
          <p className="mb-3 break-words font-mono text-[12px] text-ink-dim">{this.state.error.message}</p>
          <div className="flex gap-2">
            <button
              className="rounded-lg bg-accent px-3 py-1.5 font-semibold text-accent-ink"
              onClick={() => {
                clearTimeout(this.#timer)
                this.setState({ error: null })
              }}
            >
              Try again
            </button>
            {!window.location.pathname.includes('launcher') && (
              <button className="rounded-lg bg-panel-raised px-3 py-1.5 font-semibold" onClick={() => void backend.closeOverlay().catch(() => {})}>
                Close map
              </button>
            )}
            <button className="rounded-lg bg-panel-raised px-3 py-1.5 font-semibold" onClick={() => void saveProblemReport().catch((e: unknown) => logUiError(`could not save a report: ${String(e)}`))}>
              Save a problem report
            </button>
          </div>
        </div>
      </div>
    )
  }
}
