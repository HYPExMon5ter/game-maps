import { AlertTriangle, CheckCircle2, RefreshCw, XCircle } from 'lucide-react'
import type { ReactNode } from 'react'
import { checkLines, type LineTone } from '../lib/sync/connectionCheck'
import { runConnectionCheck, useSyncStore } from '../lib/sync/sync'
import { gameSettings } from '../settings/model'
import { useSettings } from '../settings/store'
import { useStore } from '../state/store'
import { Button } from './controls'

const TONES: Record<LineTone, { Icon: typeof CheckCircle2; className: string }> = {
  ok: { Icon: CheckCircle2, className: 'text-live' },
  warn: { Icon: AlertTriangle, className: 'text-amber-300' },
  bad: { Icon: XCircle, className: 'text-danger' },
}

/** Tests the way to the player's friends and says what's in it. */
export function ConnectionCheck({ label, hint, gameId }: { label: ReactNode; hint?: string; gameId: string | undefined }) {
  const playTogether = useSettings((s) => gameSettings(s.ui, gameId).playTogether)
  const others = useStore((s) => s.snapshot?.others)
  const session = useSyncStore((s) => s.session)
  const peers = useSyncStore((s) => s.peers)
  const failures = useSyncStore((s) => s.failures)
  const report = useSyncStore((s) => s.report)
  const checking = useSyncStore((s) => s.checking)
  const lines = report ? checkLines({ report, playTogether, session, players: others ?? [], peers: Object.values(peers), failures }) : []
  return (
    <div className="py-1">
      <span className="block text-[13px]">{label}</span>
      {hint && <span className="block pb-1.5 text-[11px] text-ink-faint">{hint}</span>}
      <Button disabled={checking} onClick={() => void runConnectionCheck()}>
        <RefreshCw className={`size-4 ${checking ? 'animate-spin' : ''}`} /> {checking ? 'Checking…' : report ? 'Check again' : 'Check connection'}
      </Button>
      {lines.length > 0 && (
        <ul className="mt-2 flex flex-col gap-1.5">
          {lines.map(({ tone, title, detail }, i) => {
            const { Icon, className } = TONES[tone]
            return (
              <li key={i} className="flex gap-2 text-[12px] leading-snug">
                <Icon className={`mt-px size-4 shrink-0 ${className}`} />
                <span>
                  <span className="block">{title}</span>
                  {detail && <span className="block text-ink-faint">{detail}</span>}
                </span>
              </li>
            )
          })}
        </ul>
      )}
    </div>
  )
}
