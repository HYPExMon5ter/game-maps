// Controls of the underground 3D view: which cave to look at, how much to cut away from
// above (to see the levels under the one you're on), and the ground surface.

import { Box, Info, RotateCcw, Square } from 'lucide-react'
import type { Map3D } from '../lib/map3d'
import { useStore } from '../state/store'
import { AUTO_CLIP_ABOVE, useView3D, type Clip } from '../state/view3d'
import { Segmented, Switch } from './controls'

const HELP = 'Drag to turn · Right-drag or Shift-drag to move · Scroll to zoom · Double-click to look at a spot · Right-click for the menu'
/** The controls, shown in the bar so nobody has to find them. */
export const SHORT_HELP = 'Drag to turn · Right-drag to move'

const VIEW_BUTTON = 'flex items-center gap-1 rounded-md px-1.5 py-1 text-ink-dim hover:bg-white/10 hover:text-ink'

export function Cave3DControls({ data }: { data: Map3D }) {
  const { clip, ground, current, setClip, setGround, focusMesh, tiltTo } = useView3D()
  const up = useStore((s) => s.map?.projection.up ?? 'y')
  const playerUp = useStore((s) => (s.snapshot?.local?.layer === 'caves' ? (s.snapshot.local.position[up] ?? null) : null))
  const unitsPerMeter = useStore((s) => s.game?.unitsPerMeter ?? 1)
  const mesh = data.meshes.find((m) => m.id === current) ?? null
  const axis = { x: 0, y: 1, z: 2 }[up]
  const low = mesh ? Math.floor(mesh.min[axis]) : 0
  const high = mesh ? Math.ceil(mesh.max[axis]) : 0
  const sorted = [...data.meshes].sort((a, b) => a.name.localeCompare(b.name, undefined, { numeric: true }))

  const setMode = (mode: Clip['mode']) => {
    if (mode !== 'height') return setClip({ mode })
    // Start the slider where the cut is now, else halfway down the cave in view.
    const from = clip.mode === 'auto' && playerUp !== null ? playerUp + AUTO_CLIP_ABOVE * unitsPerMeter : (low + high) / 2
    setClip({ mode: 'height', height: Math.round(Math.min(Math.max(from, low), high)) })
  }

  return (
    <div className="glass mb-1 flex flex-wrap items-center gap-x-3 gap-y-2 rounded-2xl px-3 py-2 text-[12.5px]">
      <select
        value={current ?? ''}
        onChange={(e) => e.target.value && focusMesh(e.target.value)}
        aria-label="Cave or bunker to look at"
        className="max-w-[180px] rounded-md bg-black/30 px-1.5 py-1 text-[12.5px] text-ink outline-none"
      >
        {!current && <option value="">Pick a cave…</option>}
        {sorted.map((m) => (
          <option key={m.id} value={m.id} className="bg-panel-solid">
            {m.name}
          </option>
        ))}
      </select>

      <div className="flex items-center gap-2">
        <span className="text-ink-dim">Cut away</span>
        <Segmented
          label="Cut away from above"
          value={clip.mode}
          onChange={setMode}
          options={[
            { id: 'auto', label: 'Above me', title: 'Hide what is above your level while you are underground' },
            { id: 'height', label: 'Height', title: 'Hide everything above a height you pick' },
            { id: 'off', label: 'Nothing', title: 'Show every level' },
          ]}
        />
        {clip.mode === 'height' && mesh && (
          <input
            type="range"
            aria-label="Cut height"
            title={`Cut at height ${Math.round(clip.height)}`}
            className="w-32 accent-[var(--color-accent)]"
            min={low}
            max={high}
            step={0.5}
            value={Math.min(Math.max(clip.height, low), high)}
            onChange={(e) => setClip({ mode: 'height', height: Number(e.target.value) })}
          />
        )}
      </div>

      <label className="flex cursor-pointer items-center gap-2">
        <span className="text-ink-dim">Ground</span>
        <Switch checked={ground} onChange={setGround} label="Show the ground surface" />
      </label>

      <div className="flex items-center gap-1">
        <button type="button" className={VIEW_BUTTON} title="Look straight down, north up" onClick={() => tiltTo('top')}>
          <Square className="size-3.5" /> Top
        </button>
        <button type="button" className={VIEW_BUTTON} title="Look at an angle, to see the levels" onClick={() => tiltTo('angled')}>
          <Box className="size-3.5" /> Angled
        </button>
      </div>

      <button
        type="button"
        className={`${VIEW_BUTTON} disabled:opacity-40`}
        disabled={!current}
        title="Back to the whole cave"
        onClick={() => current && focusMesh(current)}
      >
        <RotateCcw className="size-3.5" /> Reset view
      </button>

      <span className="flex items-center gap-1.5 text-ink-faint" title={HELP}>
        <Info className="size-3.5" /> {SHORT_HELP}
      </span>
    </div>
  )
}
