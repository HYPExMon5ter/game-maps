import { Navigation, Users, X } from 'lucide-react'
import { acceptRequest, dismissRequest, useSyncStore } from '../lib/sync/sync'
import { Button } from './controls'

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

/** While playing (the HUD can't be clicked): a note that a request waits on the map. */
export function SyncRequestNotice() {
  const latest = useSyncStore((s) => s.requests.at(-1))
  if (!latest) return null
  return (
    <div className="glass rounded-xl px-3 py-2 text-[12px] leading-snug">
      <span className="font-semibold">{latest.from}</span> wants you to track <span className="font-semibold">{latest.target.name}</span>
      <span className="text-ink-faint"> · open the map to accept</span>
    </div>
  )
}
