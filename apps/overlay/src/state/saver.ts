// Writes changes to disk a moment after the last one, and keeps trying when that fails.

export interface Saver {
  /** Something changed: save soon. */
  touch(): void
}

export interface SaverOptions {
  /** How long after the last change to save. */
  delayMs: number
  /** How long after a failed save to try again. */
  retryMs: number
  /** Called once when saving starts to fail, not again until a save has worked. */
  onFailure(error: unknown): void
}

export function createSaver(save: () => Promise<void>, { delayMs, retryMs, onFailure }: SaverOptions): Saver {
  let timer: ReturnType<typeof setTimeout> | undefined
  let saving = false
  let failing = false
  // Changes not yet in a save that worked. One made while a save runs may have missed it.
  let unsaved = false

  const schedule = (ms: number) => {
    clearTimeout(timer)
    timer = setTimeout(() => void run(), ms)
  }

  const run = async () => {
    timer = undefined
    if (saving) return
    saving = true
    unsaved = false
    try {
      await save()
      failing = false
    } catch (e) {
      unsaved = true
      if (!failing) onFailure(e)
      failing = true
    }
    saving = false
    if (unsaved && timer === undefined) schedule(failing ? retryMs : delayMs)
  }

  return {
    touch() {
      unsaved = true
      schedule(delayMs)
    },
  }
}
