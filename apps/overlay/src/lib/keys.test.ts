import { describe, expect, it } from 'vitest'
import { hotkeyLabel, vkFromCode, vkLabel } from './keys'

describe('keys', () => {
  it('maps letters, digits and function keys to VK codes', () => {
    expect(vkFromCode('KeyM')).toBe(0x4d)
    expect(vkFromCode('Digit7')).toBe(0x37)
    expect(vkFromCode('F1')).toBe(0x70)
    expect(vkFromCode('F12')).toBe(0x7b)
    expect(vkFromCode('Numpad5')).toBe(0x65)
    expect(vkFromCode('Backquote')).toBe(0xc0)
    expect(vkFromCode('NumpadAdd')).toBe(0x6b)
    expect(vkFromCode('NumpadSubtract')).toBe(0x6d)
    expect(vkFromCode('Equal')).toBe(0xbb)
    expect(vkFromCode('Minus')).toBe(0xbd)
    expect(vkFromCode('ShiftLeft')).toBeNull()
  })

  it('labels VK codes', () => {
    expect(vkLabel(0x4d)).toBe('M')
    expect(vkLabel(0x70)).toBe('F1')
    expect(vkLabel(0xc0)).toBe('Backquote')
    expect(vkLabel(0x6b)).toBe('Num +')
    expect(vkLabel(0xbd)).toBe('-')
    expect(hotkeyLabel({ action: 'toggle_map', vk: 0x4d, ctrl: true, shift: true })).toBe('Ctrl + Shift + M')
  })
})
