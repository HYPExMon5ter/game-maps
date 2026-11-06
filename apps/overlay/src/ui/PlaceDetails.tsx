// What the game's map says about a place: how much is scavenged and what's there. Asked for
// when a place's pin opens, so it costs the game nothing the rest of the time.

import { ChevronDown } from 'lucide-react'
import { useEffect, useState } from 'react'
import { backend } from '../lib/backend'
import type { PlaceInfo, Vec3 } from '../lib/protocol'

export function PlaceDetails({ position }: { position: Vec3 }) {
  const [place, setPlace] = useState<PlaceInfo | null>(null)
  useEffect(() => {
    let current = true
    // Not every pin in a place's category is near one: then there's nothing to add.
    backend.placeInfo(position).then(
      (info) => current && setPlace(info),
      () => {},
    )
    return () => {
      current = false
    }
  }, [position])
  if (!place) return null
  const { scavenged } = place
  return (
    <div className="mt-3 space-y-2.5 border-t border-line pt-3">
      <div className="flex justify-between text-[12px] text-ink-faint">
        <span>Level {place.level}</span>
        {scavenged !== null && <span className="tabular-nums text-ink-dim">{scavenged >= 100 ? 'Cleared' : `${scavenged}% scavenged`}</span>}
      </div>
      {scavenged !== null && (
        <div className="h-1 overflow-hidden rounded-full bg-black/30">
          <div className="h-full bg-accent" style={{ width: `${Math.min(scavenged, 100)}%` }} />
        </div>
      )}
      {place.loot.map((group) => (
        <details key={group.title} className="group">
          <summary className="flex cursor-pointer list-none items-center justify-between text-[12px] text-ink-dim hover:text-ink">
            {group.title}
            <span className="flex items-center gap-1 text-[11px] text-ink-faint">
              {group.items.length}
              <ChevronDown className="size-3.5 transition-transform group-open:rotate-180" />
            </span>
          </summary>
          <div className="scroll-thin mt-1.5 flex max-h-40 flex-wrap gap-1 overflow-y-auto pr-1">
            {group.items.map((item) => (
              <span key={item.name} className={`rounded-md bg-black/30 px-1.5 py-0.5 text-[11.5px] ${item.taken ? 'text-ink-faint line-through' : 'text-ink-dim'}`}>
                {item.name}
              </span>
            ))}
          </div>
        </details>
      ))}
    </div>
  )
}
