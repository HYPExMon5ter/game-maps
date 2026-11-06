import { BiomeTools } from './BiomeTools'
import { MapPinPlus, Search, X } from 'lucide-react'
import { useMemo, useState, type MouseEvent as ReactMouseEvent } from 'react'
import { customPinAtPlayer, flyToPin } from '../lib/actions'
import { formatDistance } from '../lib/compass'
import { iconUrl, LAYER_LABELS, type BadgeShape, type Category, type Pin } from '../lib/game'
import { glyphColor } from '../map/badges'
import { groundDistance } from '../lib/projection'
import { PinSearch } from '../lib/search'
import { EMPTY_USER_DATA, selectAllPins, useStore } from '../state/store'
import { CategoryMatches, CategoryTree, OnlyBanner } from './CategoryTree'
import { Button, Switch } from './controls'
import { FilterSets } from './FilterSets'
import { Progress, UnexploredNote } from './Progress'
import { usePinFog } from '../state/fog'
import { useSettings } from '../settings/store'

/** Badge shapes that aren't a rounded box, matching the ones on the map. */
const BADGE_CLIPS: Partial<Record<BadgeShape, string>> = {
  diamond: 'polygon(50% 0, 100% 50%, 50% 100%, 0 50%)',
  hexagon: 'polygon(50% 0, 94% 25%, 94% 75%, 50% 100%, 6% 75%, 6% 25%)',
  shield: 'polygon(6% 4%, 94% 4%, 94% 60%, 50% 100%, 6% 60%)',
}

export function CategoryIcon({ gameId, category, icon, color, size = 22 }: { gameId: string; category?: Category; icon?: string; color?: string; size?: number }) {
  const ref = icon ?? category?.icon ?? 'builtin:pin'
  const url = iconUrl(gameId, ref)
  const user = ref.startsWith('user:')
  const background = color ?? category?.color ?? '#ffffff'
  const darkGlyph = glyphColor(background) !== '#ffffff'
  const shape = category?.shape ?? 'circle'
  const outline = shape === 'circle' ? 'rounded-full ring-1 ring-white/70' : shape === 'square' ? 'rounded-[22%] ring-1 ring-white/70' : ''
  return (
    <span
      className={`inline-grid shrink-0 place-items-center overflow-hidden ${outline}`}
      style={{ width: size, height: size, background, clipPath: BADGE_CLIPS[shape] }}
    >
      {url ? (
        <img
          src={url}
          alt=""
          className={user ? 'size-full object-cover' : `size-[58%] object-contain ${darkGlyph ? 'brightness-0 opacity-85' : ''}`}
          draggable={false}
        />
      ) : (
        <span className={`size-[35%] rounded-full ${darkGlyph ? 'bg-[#1d232a]' : 'bg-white'}`} />
      )}
    </span>
  )
}

export function Sidebar({ onEditPin, onPinMenu }: { onEditPin: (pin: Pin) => void; onPinMenu: (pin: Pin, e: ReactMouseEvent) => void }) {
  const game = useStore((s) => s.game)
  const gamePins = useStore((s) => s.gamePins)
  const user = useStore((s) => s.user)
  const query = useStore((s) => s.query)
  const setQuery = useStore((s) => s.setQuery)
  const showFound = useStore((s) => s.showFound)
  const setShowFound = useStore((s) => s.setShowFound)
  const setAllCategories = useStore((s) => s.setAllCategories)
  const selectPin = useStore((s) => s.selectPin)
  const map = useStore((s) => s.map)
  const hasLocation = useStore((s) => !!s.snapshot?.local)
  // Not subscribed: distances in the results don't need the sidebar redrawn 30 times a second.
  const local = useStore.getState().snapshot?.local ?? null
  const units = useSettings((s) => s.ui.units)
  const [view, setView] = useState<'categories' | 'progress'>('categories')

  const friendPins = useStore((s) => s.friendPins)
  const customPins = user.customPins
  const pins = useMemo(() => selectAllPins({ gamePins, user: { ...EMPTY_USER_DATA, customPins }, friendPins }), [gamePins, customPins, friendPins])
  // Built when it's needed: pins change while exploring, and most of the time nothing's searched.
  const searching = query.trim().length > 0
  const index = useMemo(() => (game && searching ? new PinSearch(pins, game.categories) : null), [pins, game, searching])
  const hiddenByFog = usePinFog()
  const gameMarkers = useStore((s) => (s.snapshot?.markers ?? []).filter((m) => m.kind !== 'companion').length)
  const search = useMemo(() => {
    if (!index || !query.trim()) return null
    const byId = new Map(pins.map((p) => [p.id, p]))
    const matches = index
      .search(query)
      .map((id) => byId.get(id))
      .filter((p): p is Pin => !!p)
    const shown = hiddenByFog ? matches.filter((p) => !hiddenByFog(p)) : matches
    return { results: shown.slice(0, 200), unexplored: matches.length - shown.length }
  }, [index, query, pins, hiddenByFog])
  const results = search?.results ?? null

  if (!game) return null
  const categories = new Map(game.categories.map((c) => [c.id, c]))

  const open = (pin: Pin) => {
    selectPin(pin.id)
    flyToPin(pin)
  }

  return (
    <aside className="glass flex h-full w-[300px] shrink-0 flex-col rounded-2xl">
      <div className="p-3 pb-2">
        <div className="relative">
          <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-ink-faint" />
          <input
            id="pin-search"
            type="search"
            autoComplete="off"
            spellCheck={false}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={`Search ${pins.length.toLocaleString()} locations…  ( / )`}
            className="w-full rounded-xl border border-line bg-black/30 py-2 pl-9 pr-8 text-[13px] outline-none placeholder:text-ink-faint focus:border-accent/60"
            onKeyDown={(e) => {
              e.stopPropagation()
              if (e.key === 'Enter' && results?.[0]) open(results[0])
              if (e.key === 'Escape') {
                setQuery('')
                e.currentTarget.blur()
              }
            }}
          />
          {query && (
            <button className="absolute right-2 top-1/2 -translate-y-1/2 text-ink-faint hover:text-ink" onClick={() => setQuery('')} aria-label="Clear search">
              <X className="size-4" />
            </button>
          )}
        </div>
      </div>

      <div className="scroll-thin min-h-0 flex-1 overflow-y-auto px-2 pb-2">
        <OnlyBanner />
        {results ? null : (
          <div className="mb-1 flex rounded-lg bg-black/30 p-0.5 text-[12px] font-semibold" role="tablist" aria-label="Sidebar view">
            {(['categories', 'progress'] as const).map((v) => (
              <button
                key={v}
                type="button"
                role="tab"
                aria-selected={view === v}
                className={`flex-1 rounded-md py-1 ${view === v ? 'bg-white/15 text-ink' : 'text-ink-dim hover:text-ink'}`}
                onClick={() => setView(v)}
              >
                {v === 'categories' ? 'Categories' : 'Progress'}
              </button>
            ))}
          </div>
        )}
        {results ? (
          <>
          <CategoryMatches pins={pins} gameMarkers={gameMarkers} query={query} />
          <ul>
            {results.length === 0 && !search?.unexplored && <li className="px-2 py-6 text-center text-[13px] text-ink-faint">No matches</li>}
            {results.map((pin) => {
              const cat = categories.get(pin.category)
              const dist = local && map ? groundDistance(map.projection, local.position, pin.position) / game.unitsPerMeter : null
              return (
                <li key={pin.id}>
                  <button className="flex w-full items-center gap-2.5 rounded-lg px-2 py-1.5 text-left hover:bg-panel-raised" onClick={() => open(pin)} onContextMenu={(e) => onPinMenu(pin, e)}>
                    <CategoryIcon gameId={game.id} category={cat} icon={pin.icon} color={pin.color} />
                    <span className="min-w-0 flex-1">
                      <span className={`block truncate text-[13px] ${user.found.includes(pin.id) ? 'text-ink-faint line-through' : ''}`}>{pin.name}</span>
                      <span className="block truncate text-[11px] text-ink-faint">
                        {cat?.name ?? pin.category}
                        {pin.layer && pin.layer !== 'surface' ? ` · ${LAYER_LABELS[pin.layer]}` : ''}
                      </span>
                    </span>
                    {dist !== null && <span className="text-[11px] tabular-nums text-ink-faint">{formatDistance(dist, units)}</span>}
                  </button>
                </li>
              )
            })}
            {!!search?.unexplored && <UnexploredNote count={search.unexplored} />}
          </ul>
          </>
        ) : view === 'progress' ? (
          <Progress onOpen={open} />
        ) : (
          <>
            <div className="flex items-center justify-between px-2 pb-1 pt-1">
              <span className="text-[11px] font-semibold uppercase tracking-wider text-ink-faint">Categories</span>
              <span className="flex gap-2 text-[11px]">
                <button className="text-ink-dim hover:text-ink" onClick={() => setAllCategories(true)}>All</button>
                <button className="text-ink-dim hover:text-ink" onClick={() => setAllCategories(false)}>None</button>
              </span>
            </div>
            <FilterSets />
            <CategoryTree pins={pins} gameMarkers={gameMarkers} />
            <BiomeTools />
            {user.customPins.length > 0 && (
              <>
                <div className="px-2 pb-1 pt-3 text-[11px] font-semibold uppercase tracking-wider text-ink-faint">My pins</div>
                <ul>
                  {user.customPins.map((pin) => (
                    <li key={pin.id}>
                      <button className="flex w-full items-center gap-2.5 rounded-lg px-2 py-1.5 text-left hover:bg-panel-raised" onClick={() => open(pin)} onContextMenu={(e) => onPinMenu(pin, e)}>
                        <CategoryIcon gameId={game.id} category={categories.get(pin.category)} icon={pin.icon} color={pin.color} />
                        <span className="flex-1 truncate text-[13px]">{pin.name}</span>
                      </button>
                    </li>
                  ))}
                </ul>
              </>
            )}
          </>
        )}
      </div>

      <div className="flex items-center justify-between gap-2 border-t border-line p-3">
        <label className="flex cursor-pointer items-center gap-2 text-[12px] text-ink-dim">
          <Switch checked={showFound} onChange={setShowFound} />
          Show found
        </label>
        <Button
          disabled={!hasLocation}
          title={hasLocation ? 'Drop a pin where you are standing' : 'Join a game to pin your location'}
          onClick={() => {
            const pin = customPinAtPlayer()
            if (pin) onEditPin(pin)
          }}
        >
          <MapPinPlus className="size-4" /> Pin here
        </Button>
      </div>
    </aside>
  )
}
