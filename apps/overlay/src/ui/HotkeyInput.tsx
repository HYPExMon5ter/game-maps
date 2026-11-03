// Click-to-rebind key field, shared by the launcher and the map's settings.

import { X } from 'lucide-react'
import { useEffect, useState } from 'react'
import { hotkeyLabel, vkFromCode } from '../lib/keys'
import type { Hotkey, HotkeyAction } from '../lib/protocol'
import { Button } from './controls'

export function HotkeyInput({
  value,
  action = 'toggle_map',
  isDefault = true,
  onChange,
  onReset,
  onClear,
  label,
}: {
  /** Current binding (the override, or the default); null when turned off. */
  value: Hotkey | null
  action?: HotkeyAction
  isDefault?: boolean
  onChange: (hotkey: Hotkey) => void
  /** Goes back to the default; offered while `isDefault` is false. */
  onReset?: () => void
  /** Turns the binding off; offered while one is set. */
  onClear?: () => void
  /** Accessible name of the key field. */
  label?: string
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
      onChange({ action, vk, ctrl: e.ctrlKey, shift: e.shiftKey, alt: e.altKey })
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
  }, [capturing, onChange, action])

  return (
    <div className="flex gap-1.5">
      <Button
        tone={capturing ? 'primary' : 'default'}
        className="min-w-24"
        title="Click, then press the key you want (Esc cancels)"
        aria-label={label}
        onClick={() => setCapturing((c) => !c)}
      >
        {capturing ? 'Press a key…' : value ? hotkeyLabel(value) : 'Off'}
      </Button>
      {onClear && value && !capturing && (
        <Button title="Turn this key off" aria-label={label ? `Turn off ${label}` : 'Turn off'} onClick={onClear}>
          <X className="size-3.5" />
        </Button>
      )}
      {onReset && !isDefault && !capturing && <Button onClick={onReset}>Reset</Button>}
    </div>
  )
}
