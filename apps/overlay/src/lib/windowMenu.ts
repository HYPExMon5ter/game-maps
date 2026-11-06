// Keeps the keys that open a window's system menu away from Windows. Unhandled, Alt or F10
// reach `DefWindowProc`, which enters the menu loop of a window with no menu to show: the
// app's main thread and the whole webview then wait there, and the overlay freezes.

const MENU_KEYS = new Set(['Alt', 'F10'])

/** Whether `key` would start the system menu. */
export function isMenuKey(key: string): boolean {
  return MENU_KEYS.has(key)
}

export function blockWindowMenu(target: Pick<Window, 'addEventListener'> = window) {
  const block = (e: KeyboardEvent) => {
    if (isMenuKey(e.key)) e.preventDefault()
  }
  target.addEventListener('keydown', block)
  target.addEventListener('keyup', block)
}
