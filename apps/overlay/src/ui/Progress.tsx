// Completion for the current world: found collectibles per category with what's left,
// and how much of the map has been explored (see `state/fog`).

import { CheckCircle2, ChevronDown, Circle } from 'lucide-react'
import { useMemo, useState } from 'react'
import { checklistRows, type ChecklistEntry } from '../lib/checklist'
import { formatDistance } from '../lib/compass'
import { completion, percent, type CategoryProgress } from '../lib/completion'
import { exploredFraction } from '../lib/fog'
import { LAYER_LABELS, type Pin } from '../lib/game'
import { groundDistance } from '../lib/projection'
import { useSettings } from '../settings/store'
import { useFogStore, usePinFog } from '../state/fog'
import { useStore } from '../state/store'
import { worldLabel } from '../state/worlds'
import { CategoryIcon } from './Sidebar'

function Bar({ value, color }: { value: number; color?: string }) {
  return (
    <div className="h-1.5 overflow-hidden rounded-full bg-black/40" role="presentation">
      <div className="h-full rounded-full" style={{ width: `${Math.round(value * 100)}%`, background: color ?? 'var(--color-accent)' }} />
    </div>
  )
}

export function Progress({ onOpen }: { onOpen: (pin: Pin) => void }) {
  const game = useStore((s) => s.game)
  const gamePins = useStore((s) => s.gamePins)
  const found = useStore((s) => s.user.found)
  const world = useStore((s) => s.world)
  const fog = useFogStore((s) => s.fog)
  // Re-render when the fog changes (it's updated in place).
  useFogStore((s) => s.version)
  const fogEnabled = useSettings((s) => s.ui.fogOfWar)
  const progress = useMemo(() => (game ? completion(gamePins, game.categories, found) : null), [game, gamePins, found])
  // Counting the bits takes well under a millisecond.
  const explored = fog ? exploredFraction(fog) : null
  const [open, setOpen] = useState<string | null>(null)
  if (!game || !progress) return null

  return (
    <div className="px-2 pb-2">
      <div className="space-y-2 rounded-xl bg-panel-raised px-3 py-2.5">
        <div className="flex items-baseline justify-between gap-2">
          <span className="truncate text-[12px] text-ink-dim" title="Progress is kept per world">
            {worldLabel(world)}
          </span>
          <span className="font-display text-[18px] font-semibold tabular-nums">{percent(progress.found, progress.total)}%</span>
        </div>
        <Bar value={progress.total ? progress.found / progress.total : 0} />
        <div className="flex justify-between text-[11px] text-ink-faint">
          <span>
            {progress.found.toLocaleString()} of {progress.total.toLocaleString()} found
          </span>
          {fogEnabled && explored !== null && <span>{(explored * 100).toFixed(explored < 0.1 ? 1 : 0)}% of the map explored</span>}
        </div>
      </div>

      {game.checklist && <Checklist title={game.checklistName ?? 'Key items'} entries={game.checklist} pins={gamePins} found={found} onOpen={onOpen} />}

      <ul className="mt-2">
        {progress.categories.map((c) => (
          <CategoryRow key={c.category.id} gameId={game.id} progress={c} open={open === c.category.id} onToggle={() => setOpen(open === c.category.id ? null : c.category.id)} onOpen={onOpen} />
        ))}
        {progress.categories.length === 0 && <li className="px-2 py-6 text-center text-[13px] text-ink-faint">Nothing to collect on this map.</li>}
      </ul>
    </div>
  )
}

/** The game's key items: what's done, and the way to what isn't. */
function Checklist({ title, entries, pins, found, onOpen }: { title: string; entries: ChecklistEntry[]; pins: Pin[]; found: string[]; onOpen: (pin: Pin) => void }) {
  const progress = useStore((s) => s.snapshot?.progress)
  const rows = useMemo(() => checklistRows(entries, pins, found, progress), [entries, pins, found, progress])
  const [open, setOpen] = useState(false)
  const done = rows.filter((r) => r.done).length
  return (
    <div className="mt-2 rounded-xl bg-panel-raised px-2 py-1.5">
      <button className="flex w-full items-center justify-between gap-2 px-1 py-0.5 text-[13px]" aria-expanded={open} onClick={() => setOpen(!open)}>
        {title}
        <span className="flex items-center gap-1.5 text-[11px] tabular-nums text-ink-faint">
          {done}/{rows.length}
          <ChevronDown className={`size-3.5 transition-transform ${open ? 'rotate-180' : ''}`} />
        </span>
      </button>
      {open && (
        <ul className="mt-1">
          {rows.map((row) => (
            <li key={row.name}>
              <button
                className="flex w-full items-center gap-2 rounded-lg px-1 py-1 text-left text-[12.5px] hover:bg-black/20 disabled:hover:bg-transparent"
                disabled={row.pins.length === 0}
                title={row.pins.length ? 'Show on the map' : "This map doesn't have it"}
                onClick={() => onOpen(row.pins.find((p) => !found.includes(p.id)) ?? row.pins[0])}
              >
                {row.done ? <CheckCircle2 className="size-3.5 shrink-0 text-live" /> : <Circle className="size-3.5 shrink-0 text-ink-faint" />}
                <span className={`min-w-0 flex-1 truncate ${row.done ? 'text-ink-faint' : ''}`} title={row.note}>
                  {row.name}
                  {row.note && <span className="block truncate text-[11px] text-ink-faint">{row.note}</span>}
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

function CategoryRow({ gameId, progress: c, open, onToggle, onOpen }: { gameId: string; progress: CategoryProgress; open: boolean; onToggle: () => void; onOpen: (pin: Pin) => void }) {
  const done = c.found === c.total
  return (
    <li>
      <button className="flex w-full items-center gap-2.5 rounded-lg px-2 py-1.5 text-left hover:bg-panel-raised" aria-expanded={open} onClick={onToggle} disabled={done}>
        <CategoryIcon gameId={gameId} category={c.category} />
        <span className="min-w-0 flex-1">
          <span className="flex items-baseline justify-between gap-2">
            <span className="truncate text-[13px]">{c.category.name}</span>
            <span className="text-[11px] tabular-nums text-ink-faint">
              {c.found}/{c.total}
            </span>
          </span>
          <span className="mt-1 block">
            <Bar value={c.found / c.total} color={c.category.color} />
          </span>
        </span>
        {!done && <ChevronDown className={`size-3.5 shrink-0 text-ink-faint transition-transform ${open ? 'rotate-180' : ''}`} />}
      </button>
      {open && <LeftList pins={c.left} onOpen={onOpen} />}
    </li>
  )
}

/** How many pins fog of war keeps out of a list. */
export function UnexploredNote({ count }: { count: number }) {
  return (
    <li className="px-2 py-1.5 text-[11.5px] text-ink-faint" title="Explore the map (or turn off hiding pins in the fog of war settings) to see them">
      {count === 1 ? '1 more in unexplored areas' : `${count.toLocaleString()} more in unexplored areas`}
    </li>
  )
}

/** What's left in a category, nearest first when the player's position is known. */
function LeftList({ pins: all, onOpen }: { pins: Pin[]; onOpen: (pin: Pin) => void }) {
  const hiddenByFog = usePinFog()
  const pins = useMemo(() => (hiddenByFog ? all.filter((p) => !hiddenByFog(p)) : all), [all, hiddenByFog])
  const map = useStore((s) => s.map)
  const unitsPerMeter = useStore((s) => s.game?.unitsPerMeter ?? 1)
  const units = useSettings((s) => s.ui.units)
  // Sorted from where the player was when the list opened: re-sorting as they move would make it jump.
  const [origin] = useState(() => useStore.getState().snapshot?.local?.position ?? null)
  const rows = useMemo(() => {
    const withDistance = pins.map((pin) => ({ pin, dist: origin && map ? groundDistance(map.projection, origin, pin.position) / unitsPerMeter : null }))
    return withDistance.sort((a, b) => (a.dist ?? 0) - (b.dist ?? 0) || a.pin.name.localeCompare(b.pin.name))
  }, [pins, map, unitsPerMeter, origin])
  return (
    <ul className="mb-1 ml-8 border-l border-line pl-1">
      {rows.map(({ pin, dist }) => (
        <li key={pin.id}>
          <button className="flex w-full items-center gap-2 rounded-md px-2 py-1 text-left hover:bg-panel-raised" onClick={() => onOpen(pin)}>
            <span className="min-w-0 flex-1">
              <span className="block truncate text-[12.5px]">{pin.name}</span>
              {pin.layer && pin.layer !== 'surface' && <span className="block text-[11px] text-ink-faint">{LAYER_LABELS[pin.layer]}</span>}
            </span>
            {dist !== null && <span className="text-[11px] tabular-nums text-ink-faint">{formatDistance(dist, units)}</span>}
          </button>
        </li>
      ))}
      {pins.length < all.length && <UnexploredNote count={all.length - pins.length} />}
    </ul>
  )
}
