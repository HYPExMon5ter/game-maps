import { describe, expect, it, vi } from 'vitest'
import { blockWindowMenu, isMenuKey } from './windowMenu'

describe('blockWindowMenu', () => {
  it('knows the keys that start the system menu', () => {
    expect(isMenuKey('Alt')).toBe(true)
    expect(isMenuKey('F10')).toBe(true)
    expect(isMenuKey('m')).toBe(false)
  })

  it('stops them on the way down and up, and leaves other keys alone', () => {
    const handlers: Record<string, (e: KeyboardEvent) => void> = {}
    blockWindowMenu({ addEventListener: (type: string, h: never) => (handlers[type] = h) } as never)
    const press = (type: string, key: string) => {
      const preventDefault = vi.fn()
      handlers[type]({ key, preventDefault } as unknown as KeyboardEvent)
      return preventDefault.mock.calls.length
    }
    expect(press('keydown', 'Alt')).toBe(1)
    expect(press('keyup', 'Alt')).toBe(1)
    expect(press('keydown', 'Escape')).toBe(0)
  })
})
