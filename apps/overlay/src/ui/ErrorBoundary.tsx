// Keeps a UI error from taking a whole window down: logs it, shows what happened, and
// tries again on its own a few times (the overlay may be hidden, with nobody to click).

import { Component, type ErrorInfo, type ReactNode } from 'react'
import { logUiError } from '../lib/errors'

const RETRY_MS = 3000
const MAX_RETRIES = 3

export class ErrorBoundary extends Component<{ children: ReactNode }, { error: Error | null }> {
  state = { error: null as Error | null }
  #retries = 0
  #timer: ReturnType<typeof setTimeout> | undefined

  static getDerivedStateFromError(error: Error) {
    return { error }
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    logUiError(`${error.stack ?? error.message}${info.componentStack ?? ''}`)
    if (this.#retries < MAX_RETRIES) {
      this.#retries++
      this.#timer = setTimeout(() => this.setState({ error: null }), RETRY_MS)
    }
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
          <button className="rounded-lg bg-accent px-3 py-1.5 font-semibold text-accent-ink" onClick={() => this.setState({ error: null })}>
            Try again
          </button>
        </div>
      </div>
    )
  }
}
