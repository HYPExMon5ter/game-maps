// The sidebar's filters: groups that open into what's in them, each item with its own eye
// and an "Only" to see nothing else.

import { ChevronRight, Eye, EyeOff } from 'lucide-react'
import { useMemo } from 'react'
import { filterGroups, isItemVisible, type FilterGroup, type FilterItem } from '../lib/filters'
import { GAME_MARKERS_CATEGORY, type Pin } from '../lib/game'
import { useStore } from '../state/store'
import { CategoryIcon } from './Sidebar'

const MAX_MATCHES = 6

function useGroups(pins: Pin[], gameMarkers: number): FilterGroup[] {
  const game = useStore((s) => s.game)
  const found = useStore((s) => s.user.found)
  return useMemo(() => {
    if (!game) return []
    const groups = filterGroups(game, pins, new Set(found))
    // Live markers aren't pins, so they're counted apart.
    for (const group of groups) {
      for (const item of group.items) {
        if (item.key !== GAME_MARKERS_CATEGORY) continue
        item.total = gameMarkers
        group.total += gameMarkers
      }
    }
    return groups
  }, [game, pins, found, gameMarkers])
}

const counted = ({ found, total }: { found: number; total: number }) => (total === 0 ? '' : found > 0 ? `${found}/${total}` : `${total}`)

function Item({ item, gameId }: { item: FilterItem; gameId: string }) {
  const visible = useStore((s) => isItemVisible(item, s.user))
  const solo = useStore((s) => s.user.only?.key === item.key)
  const { toggleItem, showOnly, showAllAgain } = useStore.getState()
  return (
    <li className={`group/item flex items-center rounded-lg pr-2 hover:bg-panel-raised ${visible ? '' : 'opacity-45'}`}>
      <button className="flex min-w-0 flex-1 items-center gap-2 py-1.5 pl-[30px] text-left" onClick={() => toggleItem(item)} aria-pressed={visible}>
        <CategoryIcon gameId={gameId} category={item.category} icon={item.icon} color={item.color} size={16} />
        <span className="truncate text-[12.5px]">{item.label}</span>
      </button>
      <button
        className={`px-1.5 text-[11px] text-ink-dim hover:text-ink hover:underline focus:opacity-100 group-hover/item:opacity-100 ${solo ? '' : 'opacity-0'}`}
        onClick={() => (solo ? showAllAgain() : showOnly(item))}
        aria-label={`Only show ${item.label}`}
      >
        Only
      </button>
      <span className="text-[11px] tabular-nums text-ink-faint">{counted(item)}</span>
      <button className="ml-2 text-ink-faint hover:text-ink" onClick={() => toggleItem(item)} aria-label={`${visible ? 'Hide' : 'Show'} ${item.label}`}>
        {visible ? <Eye className="size-3.5" /> : <EyeOff className="size-3.5" />}
      </button>
    </li>
  )
}

function Group({ group, gameId }: { group: FilterGroup; gameId: string }) {
  const open = useStore((s) => s.user.openGroups.includes(group.id))
  const shown = useStore((s) => group.items.filter((i) => isItemVisible(i, s.user)).length)
  const { setCategoriesVisible, toggleGroupOpen } = useStore.getState()
  const partly = shown > 0 && shown < group.items.length
  return (
    <li>
      <div className={`flex items-center rounded-lg pr-2 hover:bg-panel-raised ${shown > 0 ? '' : 'opacity-45'}`}>
        <button className="flex min-w-0 flex-1 items-center gap-2 py-1.5 pl-1.5 text-left" onClick={() => toggleGroupOpen(group.id)} aria-expanded={open}>
          <ChevronRight className={`size-3.5 shrink-0 text-ink-faint transition-transform ${open ? 'rotate-90' : ''}`} />
          <CategoryIcon gameId={gameId} category={group.categories[0]} />
          <span className="truncate text-[13px]">{group.name}</span>
        </button>
        <span className="text-[11px] tabular-nums text-ink-faint">
          {[partly && `${shown} of ${group.items.length}`, counted(group)].filter(Boolean).join(' · ')}
        </span>
        <button
          className="ml-2 text-ink-faint hover:text-ink"
          onClick={() => setCategoriesVisible(group.categories.map((c) => c.id), shown === 0)}
          aria-label={`${shown > 0 ? 'Hide' : 'Show'} ${group.name}`}
        >
          {shown > 0 ? <Eye className="size-3.5" /> : <EyeOff className="size-3.5" />}
        </button>
      </div>
      {open && (
        <ul>
          {group.items.map((item) => (
            <Item key={item.key} item={item} gameId={gameId} />
          ))}
        </ul>
      )}
    </li>
  )
}

export function CategoryTree({ pins, gameMarkers }: { pins: Pin[]; gameMarkers: number }) {
  const gameId = useStore((s) => s.game?.id)
  const groups = useGroups(pins, gameMarkers)
  if (!gameId) return null
  return (
    <ul>
      {groups.map((group) => (
        <Group key={group.id} group={group} gameId={gameId} />
      ))}
    </ul>
  )
}

/** Says when the map is down to one thing, and goes back. */
export function OnlyBanner() {
  const only = useStore((s) => s.user.only)
  const showAllAgain = useStore((s) => s.showAllAgain)
  if (!only) return null
  return (
    <div className="mb-1.5 flex items-center justify-between gap-2 rounded-lg bg-accent/20 px-2.5 py-1.5 text-[12px]">
      <span className="min-w-0 truncate">
        Only showing <b className="font-semibold">{only.label}</b>
      </span>
      <button className="shrink-0 font-semibold text-ink hover:underline" onClick={showAllAgain}>
        Show all
      </button>
    </div>
  )
}

/** Filters matching a search, above the pins it found. */
export function CategoryMatches({ pins, gameMarkers, query }: { pins: Pin[]; gameMarkers: number; query: string }) {
  const gameId = useStore((s) => s.game?.id)
  const showOnly = useStore((s) => s.showOnly)
  const groups = useGroups(pins, gameMarkers)
  const text = query.trim().toLowerCase()
  const matches = groups.flatMap((group) => group.items.filter((i) => i.label.toLowerCase().includes(text) || group.name.toLowerCase().includes(text)).map((item) => ({ group, item }))).slice(0, MAX_MATCHES)
  if (!gameId || matches.length === 0) return null
  return (
    <ul className="mb-1 border-b border-line pb-1">
      {matches.map(({ group, item }) => (
        <li key={item.key}>
          <button className="flex w-full items-center gap-2.5 rounded-lg px-2 py-1 text-left hover:bg-panel-raised" onClick={() => showOnly(item)}>
            <CategoryIcon gameId={gameId} category={item.category} icon={item.icon} color={item.color} size={18} />
            <span className="min-w-0 flex-1">
              <span className="block truncate text-[12.5px]">{item.label}</span>
              <span className="block truncate text-[11px] text-ink-faint">{group.name}</span>
            </span>
            <span className="text-[11px] text-ink-dim">Only</span>
          </button>
        </li>
      ))}
    </ul>
  )
}
