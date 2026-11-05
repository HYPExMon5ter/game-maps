// Edit mode: move and resize the floating map and the mini-map by dragging them or their
// edges, or with sliders. While dragging, only the elements' styles change; settings are
// saved when the drag ends, so nothing but the element itself has to keep up.

import { Maximize2, Minimize2, RotateCcw } from 'lucide-react'
import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent, type RefObject } from 'react'
import { HUD_MARGIN } from '../lib/hudLayout'
import { anchoredRect, anchorOf, fitRect, HANDLES, MIN_MAP_PANEL, moveRect, resizeRect, scaleRect, toFractions, type Handle, type Rect, type Size } from '../lib/layout'
import { maxMinimapSize, MIN_MINIMAP_SIZE, minimapSize } from '../lib/minimap'
import { DEFAULT_SETTINGS } from '../settings/model'
import { setMinimapLayout, useGameHud, useSettings } from '../settings/store'
import { useStore } from '../state/store'
import { Button } from './controls'

const HANDLE_STYLE: Record<Handle, string> = {
  nw: 'left-0 top-0 -translate-x-1/2 -translate-y-1/2 cursor-nwse-resize',
  n: 'left-1/2 top-0 -translate-x-1/2 -translate-y-1/2 cursor-ns-resize',
  ne: 'right-0 top-0 translate-x-1/2 -translate-y-1/2 cursor-nesw-resize',
  e: 'right-0 top-1/2 translate-x-1/2 -translate-y-1/2 cursor-ew-resize',
  se: 'right-0 bottom-0 translate-x-1/2 translate-y-1/2 cursor-nwse-resize',
  s: 'left-1/2 bottom-0 -translate-x-1/2 translate-y-1/2 cursor-ns-resize',
  sw: 'left-0 bottom-0 -translate-x-1/2 translate-y-1/2 cursor-nesw-resize',
  w: 'left-0 top-1/2 -translate-x-1/2 -translate-y-1/2 cursor-ew-resize',
}

function applyRect(el: HTMLElement | null, r: Rect) {
  if (!el) return
  el.style.left = `${r.left}px`
  el.style.top = `${r.top}px`
  el.style.width = `${r.width}px`
  el.style.height = `${r.height}px`
}

/** Follows a pointer from `e` until it's released, reporting how far it has moved. */
function trackDrag(e: ReactPointerEvent, onMove: (dx: number, dy: number) => void, onEnd: () => void) {
  e.preventDefault()
  e.stopPropagation()
  const el = e.currentTarget as HTMLElement
  const { clientX: x, clientY: y, pointerId } = e
  el.setPointerCapture(pointerId)
  const move = (ev: PointerEvent) => onMove(ev.clientX - x, ev.clientY - y)
  const end = () => {
    el.removeEventListener('pointermove', move)
    el.removeEventListener('pointerup', end)
    el.removeEventListener('pointercancel', end)
    onEnd()
  }
  el.addEventListener('pointermove', move)
  el.addEventListener('pointerup', end)
  el.addEventListener('pointercancel', end)
}

/** A box the user can drag around and resize by its edges. */
function EditBox({
  boxRef,
  rect,
  screen,
  min,
  max,
  label,
  onChange,
  onCommit,
}: {
  /** What moves along with the box, besides the box. */
  boxRef?: RefObject<HTMLElement | null>
  rect: Rect
  screen: Size
  min: Size
  max: Size
  label: string
  onChange?: (r: Rect) => void
  onCommit: (r: Rect) => void
}) {
  const self = useRef<HTMLDivElement>(null)
  const begin = (e: ReactPointerEvent, next: (start: Rect, dx: number, dy: number) => Rect) => {
    let current = rect
    trackDrag(
      e,
      (dx, dy) => {
        current = next(rect, dx, dy)
        applyRect(self.current, current)
        applyRect(boxRef?.current ?? null, current)
        onChange?.(current)
      },
      () => onCommit(current),
    )
  }
  return (
    <div
      ref={self}
      className="pointer-events-auto absolute touch-none cursor-move rounded-2xl outline-2 outline-dashed outline-accent"
      style={rect}
      onPointerDown={(e) => begin(e, (s, dx, dy) => moveRect(s, dx, dy, screen))}
    >
      <span className="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 select-none whitespace-nowrap rounded-full bg-black/70 px-3 py-1 text-[13px] font-semibold text-white">
        {label}
      </span>
      {HANDLES.map((h) => (
        <span
          key={h}
          data-handle={h}
          className={`absolute size-4 rounded-full bg-accent ring-2 ring-white ${HANDLE_STYLE[h]}`}
          onPointerDown={(e) => begin(e, (s, dx, dy) => resizeRect(s, h, dx, dy, min, max, screen))}
        />
      ))}
    </div>
  )
}

/** A slider that applies its value as it moves and saves it when released. */
function SizeSlider({ label, value, min, max, onInput, onCommit }: { label: string; value: number; min: number; max: number; onInput: (v: number) => void; onCommit: () => void }) {
  const [draft, setDraft] = useState<number | null>(null)
  const shown = draft ?? value
  const commit = () => {
    if (draft === null) return
    setDraft(null)
    onCommit()
  }
  return (
    <label className="block w-[220px]">
      <span className="mb-1 flex justify-between text-[12px] text-ink-dim">
        <span>{label}</span>
        <span className="tabular-nums text-ink">{Math.round(shown * 100)}%</span>
      </span>
      <input
        type="range"
        className="w-full accent-[var(--color-accent)]"
        min={min}
        max={max}
        step={0.005}
        value={shown}
        onChange={(e) => {
          const v = Number(e.target.value)
          setDraft(v)
          onInput(v)
        }}
        onPointerUp={commit}
        onKeyUp={commit}
      />
    </label>
  )
}

/** Everything edit mode draws, above the map. `panelRef` is the floating map itself. */
export function LayoutEditor({ panelRef, panel, screen }: { panelRef: RefObject<HTMLDivElement | null>; panel: Rect; screen: Size }) {
  const { setUi } = useSettings.getState()
  const gameId = useStore((s) => s.game?.id)
  const hud = useGameHud(gameId)
  const maximized = useSettings((s) => s.ui.mapMaximized)
  const close = () => useStore.getState().setEditingLayout(false)
  const miniRef = useRef<HTMLDivElement>(null)
  const size = minimapSize(hud, screen)
  const mini = anchoredRect(hud.anchor, size, screen, HUD_MARGIN)
  const miniMax = maxMinimapSize(screen)
  const miniMin = { width: MIN_MINIMAP_SIZE, height: MIN_MINIMAP_SIZE }

  // Drafts while a slider moves: the boxes follow without saving anything.
  const panelDraft = useRef<Rect | null>(null)
  const miniDraft = useRef<Rect | null>(null)
  const savePanel = (r: Rect) => setUi({ mapPanel: toFractions(r, screen) })
  const saveMini = (r: Rect) => setMinimapLayout(gameId, { width: Math.round(r.width), height: Math.round(r.height), anchor: anchorOf(r, screen, HUD_MARGIN) })

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      e.stopPropagation()
      close()
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [])

  const reset = () => {
    setUi({ mapPanel: DEFAULT_SETTINGS.mapPanel, mapMaximized: false })
    setMinimapLayout(gameId, { anchor: DEFAULT_SETTINGS.hud.anchor, width: DEFAULT_SETTINGS.hud.width, height: DEFAULT_SETTINGS.hud.height })
  }

  return (
    <div className="pointer-events-none absolute inset-0 z-50">
      <div className="absolute inset-0 bg-black/20" />
      {!maximized && <EditBox boxRef={panelRef} rect={panel} screen={screen} min={MIN_MAP_PANEL} max={screen} label="Drag to move the map" onCommit={savePanel} />}
      <div ref={miniRef}>
        <EditBox rect={mini} screen={screen} min={miniMin} max={miniMax} label="Mini-map" onCommit={saveMini} />
      </div>
      <div className="glass pointer-events-auto absolute bottom-6 left-1/2 flex -translate-x-1/2 items-end gap-5 rounded-2xl px-5 py-3.5">
        {!maximized && (
          <SizeSlider
            label="Map size"
            value={panel.width / screen.width}
            min={Math.min(MIN_MAP_PANEL.width / screen.width, 1)}
            max={1}
            onInput={(v) => {
              const width = v * screen.width
              const r = fitRect(scaleRect(panel, width, Math.max(MIN_MAP_PANEL.height, (panel.height / panel.width) * width), screen), screen)
              panelDraft.current = r
              applyRect(panelRef.current, r)
            }}
            onCommit={() => panelDraft.current && savePanel(panelDraft.current)}
          />
        )}
        <SizeSlider
          label="Mini-map size"
          value={size.width / miniMax.width}
          min={MIN_MINIMAP_SIZE / miniMax.width}
          max={1}
          onInput={(v) => {
            const width = v * miniMax.width
            const height = Math.min(miniMax.height, Math.max(MIN_MINIMAP_SIZE, (size.height / size.width) * width))
            const r = anchoredRect(hud.anchor, { width, height }, screen, HUD_MARGIN)
            miniDraft.current = r
            applyRect(miniRef.current?.firstElementChild as HTMLElement, r)
          }}
          onCommit={() => miniDraft.current && saveMini(miniDraft.current)}
        />
        <Button onClick={() => setUi({ mapMaximized: !maximized })}>
          {maximized ? <Minimize2 className="size-4" /> : <Maximize2 className="size-4" />} {maximized ? 'Float the map' : 'Fill the screen'}
        </Button>
        <Button onClick={reset}>
          <RotateCcw className="size-4" /> Reset
        </Button>
        <Button tone="primary" onClick={close}>
          Done
        </Button>
      </div>
    </div>
  )
}
