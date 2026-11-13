// Controls of the tilted 3D surface: how to move it, and a way back to the starting view.

import { Info, RotateCcw } from 'lucide-react'
import { mapHandle } from '../map/handle'
import { START_PITCH } from '../state/view3d'

export function Surface3DControls() {
  return (
    <div className="glass mb-1 flex items-center gap-3 rounded-2xl px-3 py-2 text-[12.5px]">
      <button
        type="button"
        className="flex items-center gap-1 rounded-md px-1.5 py-1 text-ink-dim hover:bg-white/10 hover:text-ink"
        title="North up, at the starting tilt"
        onClick={() => mapHandle.current?.easeTo({ bearing: 0, pitch: START_PITCH, duration: 400 })}
      >
        <RotateCcw className="size-3.5" /> Reset view
      </button>
      <span className="flex items-center gap-1.5 text-ink-faint" title="Drag to move · Right-drag to turn and tilt · Scroll to zoom">
        <Info className="size-3.5" /> Right-drag to turn · Scroll to zoom
      </span>
    </div>
  )
}
