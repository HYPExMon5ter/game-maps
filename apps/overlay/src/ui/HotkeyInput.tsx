// Click-to-rebind key field, shared by the launcher and the map's settings.

import { useEffect, useState } from 'react'
import { hotkeyLabel, vkFromCode } from '../lib/keys'
import type { Hotkey } from '../lib/protocol'
import { Button } from './controls'

export function HotkeyInput({
  value,
  isDefault,
  onChange,
}: {
  /** Current binding (the override, or the game's default). */
  value: Hotkey | null
  isDefault: boolean
  /** A new binding, or null to go back to the game's default. */
  onChange: (hotkey: Hotkey | null) => void
}) {
  const [capturing, setCapturing] = useState(false)

  useEffect(() => {
    if (!capturing) return
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault()
      e.stopPropagation()
      if (e.key === 'Escape') return setCapturing(false)
      const vk = vkFromCode(e.code)
      if (vk === null) return // modifier on its own, or an unsupported key: keep waiting
      onChange({ action: 'toggle_map', vk, ctrl: e.ctrlKey, shift: e.shiftKey, alt: e.altKey })
      setCapturing(false)
    }
    // Clicking elsewhere cancels.
    const onBlur = () => setCapturing(false)
    window.addEventListener('keydown', onKey, { capture: true })
    window.addEventListener('blur', onBlur)
    return () => {
      window.removeEventListener('keydown', onKey, { capture: true })
      window.removeEventListener('blur', onBlur)
    }
  }, [capturing, onChange])

  return (
    <div className="flex gap-1.5">
      <Button
        tone={capturing ? 'primary' : 'default'}
        className="min-w-24"
        title="Click, then press the key you want (Esc cancels)"
        onClick={() => setCapturing((c) => !c)}
      >
        {capturing ? 'Press a key…' : value ? hotkeyLabel(value) : 'Not set'}
      </Button>
      {!isDefault && !capturing && <Button onClick={() => onChange(null)}>Reset</Button>}
    </div>
  )
}
