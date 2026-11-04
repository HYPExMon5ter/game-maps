// The world (save or server) whose pins, found items and explored areas the map shows,
// and a picker to look at another one while it isn't loaded in the game.

import { Check, ChevronDown, Globe } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { showWorld } from '../state/useWorlds'
import { useStore } from '../state/store'
import { hasData, pickerWorlds, worldKindLabel, worldLabel, type WorldInfo } from '../state/worlds'

function lastPlayed(w: WorldInfo): string | null {
  if (!w.lastPlayed) return null
  return new Date(w.lastPlayed).toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' })
}

export function WorldMenu() {
  const world = useStore((s) => s.world)
  const index = useStore((s) => s.worldIndex)
  const user = useStore((s) => s.user)
  const inGame = useStore((s) => s.snapshot?.world?.id ?? null)
  const [open, setOpen] = useState(false)
  const root = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!open) return
    const onPointer = (e: PointerEvent) => {
      if (!root.current?.contains(e.target as Node)) setOpen(false)
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      // Closes this menu, not the map.
      e.stopPropagation()
      setOpen(false)
    }
    document.addEventListener('pointerdown', onPointer)
    window.addEventListener('keydown', onKey, { capture: true })
    return () => {
      document.removeEventListener('pointerdown', onPointer)
      window.removeEventListener('keydown', onKey, { capture: true })
    }
  }, [open])
  // Games that don't report worlds only ever have the default world: nothing to show.
  if (index.worlds.length === 0 && world.id === 'default') return null
  const listed = pickerWorlds(index, world.id === 'default' ? hasData(user) : !index.claimed)
  // A new game not saved yet isn't listed, but it's the one shown.
  const worlds = listed.some((w) => w.id === world.id) ? listed : [world, ...listed]
  const locked = inGame !== null

  return (
    <div ref={root} className="relative min-w-0 shrink">
      <button
        type="button"
        className="flex max-w-[260px] items-center gap-1.5 rounded-lg px-2 py-1 text-[12px] text-ink-dim hover:bg-panel-raised hover:text-ink"
        title={locked ? 'The world loaded in the game' : 'Show another world'}
        aria-expanded={open}
        onClick={() => setOpen(!open)}
      >
        <Globe className="size-3.5 shrink-0" />
        <span className="truncate">{worldLabel(world)}</span>
        <ChevronDown className="size-3 shrink-0" />
      </button>
      {open && (
        <div className="glass absolute left-0 top-9 z-20 w-[300px] rounded-xl p-1.5" style={{ background: 'var(--color-panel-solid)' }}>
          <p className="px-2 pb-1.5 pt-1 text-[11px] text-ink-faint">
            {locked
              ? 'Each world keeps its own pins, found items and explored areas. The map follows the world loaded in the game.'
              : 'Each world keeps its own pins, found items and explored areas. Pick one to look at it.'}
          </p>
          {worlds.map((w) => {
            const current = w.id === world.id
            const details = [worldKindLabel(w.kind), w.id === inGame ? 'in the game now' : lastPlayed(w)].filter(Boolean).join(' · ')
            return (
              <button
                key={w.id}
                type="button"
                disabled={locked && !current}
                className="flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left hover:bg-panel-raised disabled:cursor-not-allowed disabled:opacity-45 disabled:hover:bg-transparent"
                onClick={() => {
                  setOpen(false)
                  if (!current) void showWorld(w.id)
                }}
              >
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-[13px]">{worldLabel(w)}</span>
                  {details && <span className="block truncate text-[11px] text-ink-faint">{details}</span>}
                </span>
                {current && <Check className="size-4 shrink-0 text-accent" />}
              </button>
            )
          })}
        </div>
      )}
    </div>
  )
}
