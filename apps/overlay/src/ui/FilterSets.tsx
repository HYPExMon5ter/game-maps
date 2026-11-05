import { BookmarkPlus, X } from 'lucide-react'
import { useState } from 'react'
import { applyFilterSet, deleteFilterSet, matchingFilterSet, saveFilterSet } from '../lib/filterSets'
import { useStore } from '../state/store'

const MAX_NAME = 32

/** Named category filters to switch between, above the categories. */
export function FilterSets() {
  const user = useStore((s) => s.user)
  const categories = useStore((s) => s.game?.categories ?? null)
  const [naming, setNaming] = useState(false)
  const [name, setName] = useState('')
  if (!categories) return null
  const active = matchingFilterSet(user.filterSets, categories, user)

  const save = () => {
    if (name.trim()) saveFilterSet(name.trim())
    setName('')
    setNaming(false)
  }

  return (
    <div className="flex flex-wrap items-center gap-1.5 px-2 pb-1.5">
      {user.filterSets.map((set) => (
        <span key={set.id} className={`group flex items-center rounded-full text-[12px] ring-1 ${set.id === active?.id ? 'bg-accent/20 text-ink ring-accent' : 'text-ink-dim ring-line hover:text-ink'}`}>
          <button className="max-w-[150px] truncate py-0.5 pl-2.5 pr-1" aria-pressed={set.id === active?.id} onClick={() => applyFilterSet(set)}>
            {set.name}
          </button>
          <button className="pr-1.5 text-ink-faint hover:text-ink" aria-label={`Delete ${set.name}`} onClick={() => deleteFilterSet(set.id)}>
            <X className="size-3" />
          </button>
        </span>
      ))}
      {naming ? (
        <input
          autoFocus
          className="min-w-0 flex-1 rounded-full bg-panel-raised px-2.5 py-0.5 text-[12px] outline-none ring-1 ring-accent"
          placeholder="Name this view"
          maxLength={MAX_NAME}
          value={name}
          onChange={(e) => setName(e.target.value)}
          onBlur={save}
          onKeyDown={(e) => {
            e.stopPropagation()
            if (e.key === 'Enter') save()
            if (e.key === 'Escape') {
              setName('')
              setNaming(false)
            }
          }}
        />
      ) : (
        <button className="flex items-center gap-1 rounded-full px-1.5 py-0.5 text-[12px] text-ink-dim hover:text-ink" title="Save the categories shown now, to come back to in one click" onClick={() => setNaming(true)}>
          <BookmarkPlus className="size-3.5" /> Save view
        </button>
      )}
    </div>
  )
}
