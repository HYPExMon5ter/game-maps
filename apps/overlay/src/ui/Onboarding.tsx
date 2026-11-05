// One-time messages: how the player wants the map to work, whether to share usage
// counts, what the dark parts of the map are, and a request to star the project.

import { Map as MapIcon, Star, X } from 'lucide-react'
import { backend } from '../lib/backend'
import { logUiError } from '../lib/errors'
import { PLAY_STYLES, withPlayStyle, type PlayStyle } from '../lib/playStyle'
import { useStarPrompt } from '../lib/starPrompt'
import { finishOnboarding, useSettings } from '../settings/store'
import { Button } from './controls'

function choose(style: PlayStyle) {
  useSettings.setState((s) => ({ ui: withPlayStyle(s.ui, style) }))
}

/** First run: how the map should work, before the player has seen it. */
export function Welcome() {
  const welcomed = useSettings((s) => s.ui.onboarding.welcomed)
  // Settings are still loading until the games are known.
  const loaded = useSettings((s) => s.games.length > 0)
  if (welcomed || !loaded) return null
  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-black/70 p-4" role="dialog" aria-modal="true" aria-labelledby="welcome-title">
      <div className="glass w-full max-w-md rounded-2xl p-5">
        <h2 id="welcome-title" className="font-display text-[17px] font-semibold">
          How do you want to play?
        </h2>
        <p className="mt-1 text-[13px] text-ink-dim">This sets up the map for you. You can change it any time in Settings.</p>
        <div className="mt-3.5 grid gap-2">
          {PLAY_STYLES.map((style) => (
            <button key={style.id} className="rounded-xl border border-line bg-panel-raised px-3.5 py-2.5 text-left transition-colors hover:border-accent hover:bg-white/[0.08]" onClick={() => choose(style)}>
              <span className="flex items-center gap-2 text-[14px] font-semibold">
                {style.title}
                {style.recommended && <span className="rounded bg-accent/20 px-1.5 py-px text-[10px] font-semibold text-accent">Recommended</span>}
              </span>
              <span className="mt-0.5 block text-[12.5px] text-ink-dim">{style.text}</span>
            </button>
          ))}
        </div>
      </div>
    </div>
  )
}

/**
 * Asks whether to share usage counts; nothing is sent until the player answers. Comes
 * after Welcome, and also reaches players from before the app had analytics.
 */
export function AnalyticsChoice() {
  const shown = useSettings((s) => s.ui.onboarding.welcomed && s.app?.analytics === null)
  const setApp = useSettings((s) => s.setApp)
  if (!shown) return null
  const answer = (share: boolean) => {
    void backend
      .setAnalytics(share)
      .then(setApp)
      .catch((e: unknown) => logUiError(`could not save the analytics choice: ${String(e)}`))
  }
  const open = (page: Promise<void>) => {
    void page.catch((e: unknown) => logUiError(`could not open the page: ${String(e)}`))
  }
  const link = 'text-accent underline underline-offset-2'
  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-black/70 p-4" role="dialog" aria-modal="true" aria-labelledby="analytics-title">
      <div className="glass w-full max-w-md rounded-2xl p-5">
        <h2 id="analytics-title" className="font-display text-[17px] font-semibold">
          Share anonymous usage counts?
        </h2>
        <p className="mt-1 text-[13px] leading-snug text-ink-dim">
          Game Maps can send{' '}
          <button className={link} onClick={() => open(backend.openPosthogPage())}>
            PostHog
          </button>{' '}
          which games you play, for how long, which features you use and what went wrong, with a random ID. It never sends your name, location, pins or worlds.
        </p>
        <p className="mt-2 text-[13px] leading-snug text-ink-dim">
          Nothing is sent unless you choose Share. You can change this any time in Settings.{' '}
          <button className={link} onClick={() => open(backend.openPrivacyPage())}>
            See everything it sends
          </button>
        </p>
        <div className="mt-3.5 grid grid-cols-2 gap-2">
          <Button onClick={() => answer(false)}>Don't share</Button>
          <Button onClick={() => answer(true)}>Share</Button>
        </div>
      </div>
    </div>
  )
}

/** The first time the map opens under fog: dark doesn't mean broken. */
export function FogTip() {
  const shown = useSettings((s) => s.ui.fogOfWar && !s.ui.onboarding.fogTip)
  const setUi = useSettings((s) => s.setUi)
  if (!shown) return null
  return (
    <div className="glass pointer-events-auto absolute left-1/2 top-16 z-20 w-[380px] max-w-[90%] -translate-x-1/2 rounded-2xl p-3.5">
      <p className="flex gap-2.5 text-[13px] leading-snug">
        <MapIcon className="mt-0.5 size-4 shrink-0 text-accent" />
        <span>
          <span className="font-semibold">Clouds hide what you haven't explored.</span> They clear as you play. You can turn this off under Settings, Full map.
        </span>
      </p>
      <div className="mt-2.5 flex justify-end gap-2">
        <Button
          onClick={() => {
            setUi({ fogOfWar: false })
            finishOnboarding('fogTip')
          }}
        >
          Show the whole map
        </Button>
        <Button tone="primary" onClick={() => finishOnboarding('fogTip')}>
          Got it
        </Button>
      </div>
    </div>
  )
}

/** Asks once for a star on GitHub. */
export function StarPrompt({ gameRunning, className }: { gameRunning: boolean; className: string }) {
  const shown = useStarPrompt(gameRunning)
  if (!shown) return null
  const star = () => {
    finishOnboarding('star')
    void backend.openProjectPage().catch((e: unknown) => logUiError(`could not open the project page: ${String(e)}`))
  }
  return (
    <div className={`glass pointer-events-auto flex w-[360px] items-center gap-2.5 rounded-2xl py-2.5 pl-3.5 pr-2.5 ${className}`}>
      <p className="min-w-0 flex-1 text-[12.5px] leading-snug text-ink-dim">
        <span className="font-semibold text-ink">Game Maps is free.</span> A star on GitHub costs nothing and really helps me out.
      </p>
      <Button tone="primary" className="shrink-0" onClick={star}>
        <Star className="size-4" /> Star
      </Button>
      <button className="shrink-0 text-ink-faint hover:text-ink" aria-label="Not now" onClick={() => finishOnboarding('star')}>
        <X className="size-4" />
      </button>
    </div>
  )
}
