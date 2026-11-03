import { ImagePlus, X } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { backend } from '../lib/backend'
import { ASSET_BASE, type Pin } from '../lib/game'
import { useStore } from '../state/store'
import { Button, Toggle } from './controls'
import { CategoryIcon } from './Sidebar'

const SWATCHES = ['#ffffff', '#ffd166', '#f2994a', '#eb5757', '#f472b6', '#bb6bd9', '#56ccf2', '#2d9cdb', '#6fcf97', '#27ae60']

function useGameIcons(gameId: string | undefined): string[] {
  const [icons, setIcons] = useState<string[]>([])
  useEffect(() => {
    if (!gameId) return
    let cancelled = false
    fetch(`${ASSET_BASE}/games/${encodeURIComponent(gameId)}/icons/game/index.json`)
      .then((r) => (r.ok ? (r.json() as Promise<string[]>) : []))
      .then((names) => !cancelled && setIcons(names.filter((n) => !/Outline/i.test(n))))
      .catch(() => {})
    return () => {
      cancelled = true
    }
  }, [gameId])
  return icons
}

export function PinEditor({ pin, onClose }: { pin: Pin; onClose: () => void }) {
  const game = useStore((s) => s.game)
  const upsert = useStore((s) => s.upsertCustomPin)
  const notify = useStore((s) => s.notify)
  const [draft, setDraft] = useState<Pin>(pin)
  const icons = useGameIcons(game?.id)
  const fileRef = useRef<HTMLInputElement>(null)
  if (!game) return null
  const category = game.categories.find((c) => c.id === draft.category)

  const upload = async (file: File) => {
    try {
      const ref = await backend.saveUserIcon(file.name, new Uint8Array(await file.arrayBuffer()))
      setDraft((d) => ({ ...d, icon: `user:${ref.replace(/^user\//, '')}` }))
    } catch (e) {
      notify(String(e), 'error')
    }
  }

  const save = () => {
    upsert({ ...draft, name: draft.name.trim() || 'Pin' })
    useStore.getState().selectPin(draft.id)
    onClose()
  }

  return (
    <div className="pointer-events-auto absolute inset-0 z-30 grid place-items-center bg-black/35" onMouseDown={onClose}>
      <div
        className="glass w-[420px] rounded-2xl p-5"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          e.stopPropagation()
          if (e.key === 'Escape') onClose()
          if (e.key === 'Enter' && (e.target as HTMLElement).tagName !== 'TEXTAREA') save()
        }}
      >
        <div className="mb-4 flex items-center gap-3">
          <CategoryIcon gameId={game.id} category={category} icon={draft.icon} color={draft.color} size={40} />
          <h2 className="font-display flex-1 text-[17px] font-semibold">{pin.custom && pin.name !== 'New pin' ? 'Edit pin' : 'New pin'}</h2>
          <button className="text-ink-faint hover:text-ink" onClick={onClose} aria-label="Close">
            <X className="size-4" />
          </button>
        </div>

        <label className="mb-3 block">
          <span className="mb-1 block text-[12px] text-ink-dim">Name</span>
          <input
            autoFocus
            value={draft.name}
            onFocus={(e) => e.target.select()}
            onChange={(e) => setDraft({ ...draft, name: e.target.value })}
            className="w-full rounded-lg border border-line bg-black/30 px-3 py-2 text-[13px] outline-none focus:border-accent/60"
          />
        </label>

        <label className="mb-3 block">
          <span className="mb-1 block text-[12px] text-ink-dim">Notes</span>
          <textarea
            rows={2}
            value={draft.description ?? ''}
            onChange={(e) => setDraft({ ...draft, description: e.target.value || undefined })}
            className="w-full resize-none rounded-lg border border-line bg-black/30 px-3 py-2 text-[13px] outline-none focus:border-accent/60"
          />
        </label>

        <label className="mb-3 block">
          <span className="mb-1 block text-[12px] text-ink-dim">Category</span>
          <select
            value={draft.category}
            onChange={(e) => setDraft({ ...draft, category: e.target.value })}
            className="w-full rounded-lg border border-line bg-panel-solid px-3 py-2 text-[13px] outline-none"
          >
            {game.categories.map((c) => (
              <option key={c.id} value={c.id}>
                {c.name}
              </option>
            ))}
          </select>
        </label>

        <div className="mb-3">
          <span className="mb-1.5 block text-[12px] text-ink-dim">Color</span>
          <div className="flex flex-wrap gap-1.5">
            <button
              className={`h-6 rounded-full px-2 text-[11px] ring-1 ${!draft.color ? 'ring-accent' : 'ring-line'}`}
              onClick={() => setDraft({ ...draft, color: undefined })}
            >
              Category
            </button>
            {SWATCHES.map((c) => (
              <button
                key={c}
                aria-label={`Color ${c}`}
                className={`size-6 rounded-full ring-2 ${draft.color === c ? 'ring-accent' : 'ring-transparent'}`}
                style={{ background: c }}
                onClick={() => setDraft({ ...draft, color: c })}
              />
            ))}
            <input
              type="color"
              aria-label="Custom color"
              value={draft.color ?? category?.color ?? '#ffffff'}
              onChange={(e) => setDraft({ ...draft, color: e.target.value })}
              className="size-6 cursor-pointer rounded-full border-0 bg-transparent p-0"
            />
          </div>
        </div>

        <div className="mb-3">
          <Toggle
            label="Share with players on this server"
            hint="Others running Game Maps in your game see it while you're both connected"
            checked={!!draft.shared}
            onChange={(shared) => setDraft({ ...draft, shared: shared || undefined })}
          />
        </div>

        <div className="mb-5">
          <span className="mb-1.5 block text-[12px] text-ink-dim">Icon</span>
          <div className="scroll-thin grid max-h-[132px] grid-cols-9 gap-1.5 overflow-y-auto">
            <button
              title="Category icon"
              className={`grid aspect-square place-items-center rounded-lg text-[10px] ring-1 ${!draft.icon ? 'ring-accent' : 'ring-line'}`}
              onClick={() => setDraft({ ...draft, icon: undefined })}
            >
              Auto
            </button>
            {icons.map((name) => (
              <button
                key={name}
                title={name}
                className={`grid aspect-square place-items-center rounded-lg bg-black/30 ring-1 ${draft.icon === `game:${name}` ? 'ring-accent' : 'ring-line'}`}
                onClick={() => setDraft({ ...draft, icon: `game:${name}` })}
              >
                <img src={`${ASSET_BASE}/games/${game.id}/icons/game/${name}.png`} alt="" className="size-[62%] object-contain" />
              </button>
            ))}
            <button
              title="Upload an image"
              className={`grid aspect-square place-items-center rounded-lg ring-1 ${draft.icon?.startsWith('user:') ? 'ring-accent' : 'ring-line'}`}
              onClick={() => fileRef.current?.click()}
            >
              <ImagePlus className="size-4 text-ink-dim" />
            </button>
          </div>
          <input
            ref={fileRef}
            type="file"
            accept="image/png,image/webp,image/jpeg,image/svg+xml"
            className="hidden"
            onChange={(e) => {
              const f = e.target.files?.[0]
              if (f) void upload(f)
              e.target.value = ''
            }}
          />
        </div>

        <div className="flex justify-end gap-2">
          <Button onClick={onClose}>Cancel</Button>
          <Button tone="primary" onClick={save}>
            Save pin
          </Button>
        </div>
      </div>
    </div>
  )
}
