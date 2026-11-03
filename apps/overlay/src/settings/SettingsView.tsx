// The settings panel, shared by the map and the launcher: categories down the side,
// a search over every setting, and a game picker for the per-game ones.

import { Search, X } from 'lucide-react'
import { useMemo, useState } from 'react'
import { applicableSettings, CATEGORIES, categoryOf, GameIcon, type CategoryId, type SettingDef, type SettingsContext } from './registry'
import { matchesQuery, queryWords } from './search'
import { useSettings } from './store'

export interface SettingsViewProps {
  window: SettingsContext['window']
  /** Game the per-game settings start on (e.g. the one being played). */
  initialGameId: string | null
  initialCategory?: CategoryId
  onClose?: () => void
  onError: (message: string) => void
}

export function SettingsView({ window, initialGameId, initialCategory = 'general', onClose, onError }: SettingsViewProps) {
  const games = useSettings((s) => s.games)
  const [category, setCategory] = useState<CategoryId>(initialCategory)
  const [query, setQuery] = useState('')
  const [pickedId, setPickedId] = useState<string | null>(null)
  const gameId = pickedId ?? initialGameId ?? games.find((g) => g.liveFeatures)?.id ?? games[0]?.id ?? null
  const game = games.find((g) => g.id === gameId) ?? null
  const ctx = useMemo<SettingsContext>(() => ({ game, window, onError }), [game, window, onError])

  const all = useMemo(() => applicableSettings(ctx), [ctx])
  const words = queryWords(query)
  const searching = words.length > 0
  const matches = searching ? all.filter((d) => matchesQuery(words, [d.label, d.hint, d.keywords, categoryOf(d.category).title])) : []
  const shown = searching ? matches : all.filter((d) => d.category === category)
  const counts = (id: CategoryId) => (searching ? matches : all).filter((d) => d.category === id).length

  return (
    <div className="flex h-full min-h-0 flex-col">
      <header className="flex items-center gap-2 border-b border-line px-3 py-2.5">
        <h2 className="font-display text-[16px] font-semibold">Settings</h2>
        <label className="ml-2 flex min-w-0 flex-1 items-center gap-2 rounded-lg bg-panel-raised px-2.5 py-1.5 focus-within:ring-1 focus-within:ring-accent/60">
          <Search className="size-3.5 shrink-0 text-ink-faint" />
          <input
            type="search"
            autoFocus
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Escape' && query) {
                e.stopPropagation()
                setQuery('')
              }
            }}
            placeholder="Search settings"
            aria-label="Search settings"
            className="min-w-0 flex-1 bg-transparent text-[13px] outline-none placeholder:text-ink-faint"
          />
        </label>
        {onClose && (
          <button className="text-ink-faint hover:text-ink" onClick={onClose} aria-label="Close settings">
            <X className="size-4" />
          </button>
        )}
      </header>

      <div className="flex min-h-0 flex-1">
        <nav className="scroll-thin flex w-36 shrink-0 flex-col gap-0.5 overflow-y-auto border-r border-line p-2" aria-label="Settings categories">
          {CATEGORIES.map((c) => {
            const n = counts(c.id)
            if (!searching && n === 0) return null
            const active = !searching && c.id === category
            return (
              <button
                key={c.id}
                type="button"
                aria-current={active ? 'page' : undefined}
                onClick={() => {
                  setQuery('')
                  setCategory(c.id)
                }}
                className={`flex items-center gap-2 rounded-lg px-2 py-1.5 text-left text-[12.5px] transition-colors ${
                  active ? 'bg-accent/20 text-ink' : 'text-ink-dim hover:bg-panel-raised hover:text-ink'
                } ${searching && n === 0 ? 'opacity-40 hover:opacity-100' : ''}`}
              >
                <c.icon className="size-3.5 shrink-0" />
                <span className="min-w-0 flex-1 truncate">{c.title}</span>
                {searching && n > 0 && <span className="text-[11px] tabular-nums text-ink-faint">{n}</span>}
              </button>
            )
          })}
        </nav>

        <div className="scroll-thin min-w-0 flex-1 overflow-y-auto px-3.5 pb-3 pt-2">
          {shown.some((d) => d.scope === 'game') && games.length > 0 && (
            <GamePicker value={gameId} games={games} onChange={setPickedId} />
          )}
          {searching ? (
            matches.length === 0 ? (
              <p className="py-8 text-center text-[13px] text-ink-faint">No settings match “{query.trim()}”.</p>
            ) : (
              CATEGORIES.map((c) => {
                const defs = matches.filter((d) => d.category === c.id)
                if (!defs.length) return null
                return (
                  <section key={c.id} className="border-b border-line py-2 last:border-0">
                    <h3 className="mb-1 text-[11px] font-semibold uppercase tracking-wider text-ink-faint">{c.title}</h3>
                    <Entries defs={defs} ctx={ctx} />
                  </section>
                )
              })
            )
          ) : (
            <section className="py-1">
              <h3 className="mb-1 font-display text-[14px] font-semibold">{categoryOf(category).title}</h3>
              <Entries defs={shown} ctx={ctx} />
            </section>
          )}
        </div>
      </div>
    </div>
  )
}

function Entries({ defs, ctx }: { defs: SettingDef[]; ctx: SettingsContext }) {
  return (
    <div className="space-y-1.5">
      {defs.map((d) => (
        <d.Control
          key={d.id}
          ctx={ctx}
          hint={d.hint}
          label={
            d.scope === 'game' ? (
              <span className="inline-flex items-center gap-1.5">
                {d.label}
                <GameIcon className="size-3 text-accent" aria-label={`Only for ${ctx.game?.name ?? 'this game'}`} />
              </span>
            ) : (
              d.label
            )
          }
        />
      ))}
    </div>
  )
}

function GamePicker({ value, games, onChange }: { value: string | null; games: { id: string; name: string }[]; onChange: (id: string) => void }) {
  return (
    <div className="mb-2 flex flex-wrap items-center gap-x-2 gap-y-1 rounded-lg bg-accent/10 px-2.5 py-1.5 text-[12px] text-ink-dim">
      <span className="flex shrink-0 items-center gap-1.5">
        Settings marked <GameIcon className="size-3.5 text-accent" aria-label="with the game icon" /> are for
      </span>
      <select
        value={value ?? ''}
        onChange={(e) => onChange(e.target.value)}
        aria-label="Game for per-game settings"
        className="min-w-[10rem] flex-1 rounded-md bg-black/30 px-1.5 py-0.5 text-[12px] font-semibold text-ink outline-none"
      >
        {games.map((g) => (
          <option key={g.id} value={g.id} className="bg-panel-solid">
            {g.name}
          </option>
        ))}
      </select>
    </div>
  )
}
