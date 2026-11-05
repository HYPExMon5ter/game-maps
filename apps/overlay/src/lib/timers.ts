// Pin timers: a countdown on a pin, with a notice when it's up.

export type PinTimers = Record<string, number>

/** Pins whose timer has run out by `now`, and the timers still running. */
export function dueTimers(timers: PinTimers, now: number): { due: string[]; running: PinTimers } {
  const due: string[] = []
  const running: PinTimers = {}
  for (const [id, endsAt] of Object.entries(timers)) {
    if (endsAt <= now) due.push(id)
    else running[id] = endsAt
  }
  return { due, running }
}

/** Time left as `1h 05m`, `12m 30s` or `45s`. */
export function formatRemaining(ms: number): string {
  const total = Math.max(0, Math.ceil(ms / 1000))
  const h = Math.floor(total / 3600)
  const m = Math.floor((total % 3600) / 60)
  const s = total % 60
  const pad = (n: number) => String(n).padStart(2, '0')
  if (h > 0) return `${h}h ${pad(m)}m`
  return m > 0 ? `${m}m ${pad(s)}s` : `${s}s`
}
