// When to ask for a star on GitHub: once, after the player has had a chance to use the
// app. Only the launcher asks, so it never comes up in a game.

import { useEffect, useState } from 'react'
import { useSettings } from '../settings/store'

/** How long the app has to be running without a game having been played. */
export const STAR_DELAY_MS = 5 * 60_000

export function starDue(runningMs: number, gameSeen: boolean): boolean {
  return gameSeen || runningMs >= STAR_DELAY_MS
}

/** Whether to ask now. `gameRunning`: a game is open right now. */
export function useStarPrompt(gameRunning: boolean): boolean {
  const pending = useSettings((s) => s.ui.onboarding.welcomed && !s.ui.onboarding.star)
  const [gameSeen, setGameSeen] = useState(false)
  const [timeUp, setTimeUp] = useState(() => starDue(performance.now(), false))
  if (gameRunning && !gameSeen) setGameSeen(true)
  useEffect(() => {
    if (timeUp) return
    const id = setTimeout(() => setTimeUp(true), STAR_DELAY_MS - performance.now())
    return () => clearTimeout(id)
  }, [timeUp])
  return pending && (timeUp || gameSeen)
}
