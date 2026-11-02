import { X } from 'lucide-react'
import { useEffect, useState } from 'react'
import { agentLogs } from '../lib/agentLogs'
import { backend } from '../lib/backend'
import { MAX_UI_SCALE, MIN_UI_SCALE, useUiScale } from '../lib/uiScale'
import { hotkeyLabel, vkFromCode } from '../lib/keys'
import type { AgentLog, Corner } from '../lib/protocol'
import { useStore } from '../state/store'
import { Button, Slider, Toggle } from './controls'

const pct = (v: number) => `${Math.round(v * 100)}%`
const CORNERS: { id: Corner; label: string }[] = [
  { id: 'top_left', label: 'Top left' },
  { id: 'top_right', label: 'Top right' },
  { id: 'bottom_left', label: 'Bottom left' },
  { id: 'bottom_right', label: 'Bottom right' },
]

/** Dev tooling: saves every loaded item and point of interest for `tools/pins`. */
function ExportWorld() {
  const connected = useStore((s) => s.status?.state === 'connected')
  const [state, setState] = useState<{ busy: boolean; result?: string }>({ busy: false })
  const run = async () => {
    setState({ busy: true })
    try {
      setState({ busy: false, result: `Saved to ${await backend.exportWorld()}` })
    } catch (e) {
      setState({ busy: false, result: `Export failed: ${String(e)}` })
    }
  }
  return (
    <div className="mt-2 flex flex-col gap-1.5">
      <Button disabled={!connected || state.busy} onClick={run}>
        {state.busy ? 'Exporting…' : 'Export world data'}
      </Button>
      {state.result && <p className="break-all text-[11px] text-ink-dim">{state.result}</p>}
    </div>
  )
}

function InterfaceScale() {
  const manual = useStore((s) => s.settings.uiScale)
  const setSettings = useStore((s) => s.setSettings)
  const current = useUiScale()
  return (
    <>
      <Toggle
        label="Auto interface size"
        hint="Scales panels and text to your screen"
        checked={manual === null}
        onChange={(auto) => setSettings({ uiScale: auto ? null : current })}
      />
      {manual !== null && (
        <Slider
          label="Interface size"
          value={manual}
          min={MIN_UI_SCALE}
          max={MAX_UI_SCALE}
          step={0.05}
          format={pct}
          onChange={(uiScale) => setSettings({ uiScale })}
        />
      )}
    </>
  )
}

function HotkeyField() {
  const game = useStore((s) => s.game)
  const hotkey = useStore((s) => s.settings.hotkey)
  const setSettings = useStore((s) => s.setSettings)
  const [capturing, setCapturing] = useState(false)
  const current = hotkey ?? game?.defaultHotkeys[0] ?? null

  useEffect(() => {
    if (!capturing) return
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault()
      e.stopPropagation()
      if (e.key === 'Escape') return setCapturing(false)
      const vk = vkFromCode(e.code)
      if (vk === null) return
      setSettings({ hotkey: { action: 'toggle_map', vk, ctrl: e.ctrlKey, shift: e.shiftKey, alt: e.altKey } })
      setCapturing(false)
    }
    window.addEventListener('keydown', onKey, { capture: true })
    return () => window.removeEventListener('keydown', onKey, { capture: true })
  }, [capturing, setSettings])

  return (
    <div className="flex items-center justify-between gap-3 py-1">
      <span className="text-[13px]">Open / close map</span>
      <div className="flex gap-1.5">
        <Button tone={capturing ? 'primary' : 'default'} onClick={() => setCapturing(true)}>
          {capturing ? 'Press a key…' : current ? hotkeyLabel(current) : 'Not set'}
        </Button>
        {hotkey && <Button onClick={() => setSettings({ hotkey: null })}>Reset</Button>}
      </div>
    </div>
  )
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="border-b border-line py-3 last:border-0">
      <h3 className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-ink-faint">{title}</h3>
      <div className="space-y-2">{children}</div>
    </section>
  )
}

export function SettingsPanel({ onClose }: { onClose: () => void }) {
  const settings = useStore((s) => s.settings)
  const setSettings = useStore((s) => s.setSettings)
  const setHud = useStore((s) => s.setHud)
  const status = useStore((s) => s.status)
  const [logs, setLogs] = useState<AgentLog[]>(agentLogs)

  useEffect(() => {
    const id = setInterval(() => setLogs(agentLogs()), 1000)
    return () => clearInterval(id)
  }, [])

  return (
    <div className="glass scroll-thin pointer-events-auto max-h-full w-[340px] overflow-y-auto rounded-2xl px-4 pb-2 pt-3">
      <div className="flex items-center justify-between">
        <h2 className="font-display text-[16px] font-semibold">Settings</h2>
        <button className="text-ink-faint hover:text-ink" onClick={onClose} aria-label="Close settings">
          <X className="size-4" />
        </button>
      </div>

      <Section title="Controls">
        <HotkeyField />
        <p className="text-[11px] text-ink-faint">The key still reaches the game, so your in-game GPS keeps working.</p>
      </Section>

      <Section title="Full map">
        <Slider label="Opacity" value={settings.opacity} min={0.4} max={1} step={0.02} format={pct} onChange={(opacity) => setSettings({ opacity })} />
        <Slider label="Size" value={settings.panelScale} min={0.5} max={1} step={0.05} format={pct} onChange={(panelScale) => setSettings({ panelScale })} />
        <InterfaceScale />
      </Section>

      <Section title="While playing">
        <Toggle label="Mini-map" hint="Click-through map in a corner of the screen" checked={settings.hud.minimap} onChange={(minimap) => setHud({ minimap })} />
        <Toggle label="Rotate with view" checked={settings.hud.rotateWithPlayer} onChange={(rotateWithPlayer) => setHud({ rotateWithPlayer })} />
        <Slider label="Mini-map size" value={settings.hud.size} min={180} max={520} step={10} format={(v) => `${v}px`} onChange={(size) => setHud({ size })} />
        <Slider label="Mini-map opacity" value={settings.hud.opacity} min={0.3} max={1} step={0.02} format={pct} onChange={(opacity) => setHud({ opacity })} />
        <div className="grid grid-cols-2 gap-1.5 pt-1">
          {CORNERS.map((c) => (
            <Button key={c.id} tone={settings.hud.corner === c.id ? 'primary' : 'default'} onClick={() => setHud({ corner: c.id })}>
              {c.label}
            </Button>
          ))}
        </div>
      </Section>

      <Section title="Diagnostics">
        <p className="text-[12px] text-ink-dim">
          {status?.state ?? 'starting'}
          {status?.pid ? ` · pid ${status.pid}` : ''}
          {status?.capabilities
            ? ` · ${Object.entries(status.capabilities)
                .filter(([, v]) => v)
                .map(([k]) => k.replace('_', ' '))
                .join(', ')}`
            : ''}
        </p>
        <pre className="scroll-thin max-h-40 overflow-auto whitespace-pre-wrap rounded-lg bg-black/40 p-2 font-mono text-[10.5px] leading-relaxed text-ink-dim">
          {logs.length ? logs.map((l) => `[${l.level}] ${l.message}`).join('\n') : 'No agent messages yet.'}
        </pre>
        <ExportWorld />
      </Section>
    </div>
  )
}
