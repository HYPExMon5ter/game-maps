// Browser key codes <-> Windows virtual-key codes, for hotkey binding.

import type { Hotkey } from './protocol'

const NAMED: Record<string, number> = {
  Tab: 0x09,
  Space: 0x20,
  Backquote: 0xc0,
  Minus: 0xbd,
  Equal: 0xbb,
  BracketLeft: 0xdb,
  BracketRight: 0xdd,
  Backslash: 0xdc,
  Semicolon: 0xba,
  Quote: 0xde,
  Comma: 0xbc,
  Period: 0xbe,
  Slash: 0xbf,
  Insert: 0x2d,
  Delete: 0x2e,
  Home: 0x24,
  End: 0x23,
  PageUp: 0x21,
  PageDown: 0x22,
  CapsLock: 0x14,
  NumpadMultiply: 0x6a,
  NumpadAdd: 0x6b,
  NumpadSubtract: 0x6d,
  NumpadDecimal: 0x6e,
  NumpadDivide: 0x6f,
}

/** Labels for keys whose code name reads poorly. */
const LABELS: Record<number, string> = {
  0xbd: '-',
  0xbb: '=',
  0x6a: 'Num *',
  0x6b: 'Num +',
  0x6d: 'Num -',
  0x6e: 'Num .',
  0x6f: 'Num /',
}

/** VK code for a `KeyboardEvent.code`, or null if unsupported as a hotkey. */
export function vkFromCode(code: string): number | null {
  let m = /^Key([A-Z])$/.exec(code)
  if (m) return m[1].charCodeAt(0)
  m = /^Digit([0-9])$/.exec(code)
  if (m) return 0x30 + Number(m[1])
  m = /^Numpad([0-9])$/.exec(code)
  if (m) return 0x60 + Number(m[1])
  m = /^F([0-9]{1,2})$/.exec(code)
  if (m && Number(m[1]) >= 1 && Number(m[1]) <= 24) return 0x6f + Number(m[1])
  return NAMED[code] ?? null
}

export function vkLabel(vk: number): string {
  if ((vk >= 0x41 && vk <= 0x5a) || (vk >= 0x30 && vk <= 0x39)) return String.fromCharCode(vk)
  if (vk >= 0x60 && vk <= 0x69) return `Num ${vk - 0x60}`
  if (vk >= 0x70 && vk <= 0x87) return `F${vk - 0x6f}`
  if (LABELS[vk]) return LABELS[vk]
  const name = Object.entries(NAMED).find(([, v]) => v === vk)?.[0]
  return name ?? `Key ${vk}`
}

export function hotkeyLabel(h: Hotkey): string {
  return [h.ctrl && 'Ctrl', h.shift && 'Shift', h.alt && 'Alt', vkLabel(h.vk)].filter(Boolean).join(' + ')
}

/** Whether a DOM keyboard event matches a hotkey binding. */
export function matchesHotkey(e: KeyboardEvent, h: Hotkey): boolean {
  return vkFromCode(e.code) === h.vk && e.ctrlKey === !!h.ctrl && e.shiftKey === !!h.shift && e.altKey === !!h.alt
}
