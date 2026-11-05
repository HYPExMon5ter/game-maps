import { Backpack, Check, MapPin, Navigation, Skull, Timer, Users, X, type LucideIcon } from 'lucide-react'
import { acceptRequest, dismissRequest, hasSyncNotice, useSyncStore, type NoticeKind } from '../lib/sync/sync'
import { Button } from './controls'

const NOTICE_ICONS: Record<NoticeKind, { Icon: LucideIcon; color: string }> = {
  done: { Icon: Check, color: 'text-live' },
  failed: { Icon: X, color: 'text-danger' },
  friends: { Icon: Users, color: 'text-accent' },
  tracking: { Icon: Navigation, color: 'text-accent' },
  pin: { Icon: MapPin, color: 'text-accent' },
  bag: { Icon: Backpack, color: 'text-live' },
  death: { Icon: Skull, color: 'text-danger' },
  timer: { Icon: Timer, color: 'text-accent' },
}

/** "Track this with me" requests from other players, to accept or dismiss. */
export function SyncRequests() {
  const requests = useSyncStore((s) => s.requests)
  if (requests.length === 0) return null
  return (
    <div className="flex flex-col gap-2">
      {requests.map((r) => (
        <div key={r.id} className="glass pointer-events-auto w-[340px] rounded-2xl p-3">
          <div className="flex items-start gap-2.5">
            <Users className="mt-0.5 size-4 shrink-0 text-accent" />
            <p className="min-w-0 flex-1 text-[13px] leading-snug">
              <span className="font-semibold">{r.from}</span> wants you to track <span className="font-semibold">{r.target.name}</span>
            </p>
            <button className="text-ink-faint hover:text-ink" onClick={() => dismissRequest(r.id)} aria-label="Dismiss">
              <X className="size-4" />
            </button>
          </div>
          <div className="mt-2.5 flex gap-2 pl-6.5">
            <Button tone="primary" onClick={() => acceptRequest(r.id)}>
              <Navigation className="size-4" /> Track
            </Button>
            <Button onClick={() => dismissRequest(r.id)}>Ignore</Button>
          </div>
        </div>
      ))}
    </div>
  )
}

/**
 * While playing, drawn over the game (which can't be clicked): track requests waiting on
 * the map, and what friends just did. Sits below the danger count at the top of the screen.
 */
export function SyncNotices() {
  const requests = useSyncStore((s) => s.requests)
  const notices = useSyncStore((s) => s.notices)
  if (!hasSyncNotice({ requests, notices })) return null
  const item = 'glass flex items-center gap-2.5 rounded-xl px-3.5 py-2 text-[13px] leading-snug'
  return (
    <div className="pointer-events-none absolute left-1/2 top-12 flex max-w-[90%] -translate-x-1/2 flex-col items-center gap-2">
      {requests.map((r) => (
        <div key={r.id} className={item}>
          <Navigation className="size-4 shrink-0 text-accent" />
          <p>
            <span className="font-semibold">{r.from}</span> wants you to track <span className="font-semibold">{r.target.name}</span>
            <span className="text-ink-faint"> · open the map to accept</span>
          </p>
        </div>
      ))}
      {notices.map((n) => {
        const { Icon, color } = NOTICE_ICONS[n.kind]
        return (
          <div key={n.id} className={item}>
            <Icon className={`size-4 shrink-0 ${color}`} />
            <p>{n.text}</p>
          </div>
        )
      })}
    </div>
  )
}
