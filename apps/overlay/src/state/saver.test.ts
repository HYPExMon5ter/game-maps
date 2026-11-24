import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createSaver } from './saver'

const options = { delayMs: 400, retryMs: 5000 }

describe('createSaver', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => vi.useRealTimers())

  it('saves once, a moment after the last change', async () => {
    const save = vi.fn(() => Promise.resolve())
    const saver = createSaver(save, { ...options, onFailure: vi.fn() })
    saver.touch()
    await vi.advanceTimersByTimeAsync(300)
    saver.touch()
    await vi.advanceTimersByTimeAsync(300)
    expect(save).not.toHaveBeenCalled()
    await vi.advanceTimersByTimeAsync(100)
    expect(save).toHaveBeenCalledTimes(1)
    await vi.advanceTimersByTimeAsync(60_000)
    expect(save).toHaveBeenCalledTimes(1)
  })

  it('keeps trying after a failed save, and says so once', async () => {
    let working = false
    const save = vi.fn(() => (working ? Promise.resolve() : Promise.reject(new Error('disk full'))))
    const onFailure = vi.fn()
    const saver = createSaver(save, { ...options, onFailure })
    saver.touch()
    await vi.advanceTimersByTimeAsync(400)
    await vi.advanceTimersByTimeAsync(5000)
    expect(save).toHaveBeenCalledTimes(2)
    expect(onFailure).toHaveBeenCalledTimes(1)
    working = true
    await vi.advanceTimersByTimeAsync(5000)
    expect(save).toHaveBeenCalledTimes(3)
    await vi.advanceTimersByTimeAsync(60_000)
    expect(save).toHaveBeenCalledTimes(3)
    // Failing again later is news again.
    working = false
    saver.touch()
    await vi.advanceTimersByTimeAsync(400)
    expect(onFailure).toHaveBeenCalledTimes(2)
  })

  it('saves again for a change made while a save was running', async () => {
    let finish: () => void = () => {}
    const save = vi.fn(() => new Promise<void>((resolve) => (finish = resolve)))
    const saver = createSaver(save, { ...options, onFailure: vi.fn() })
    saver.touch()
    await vi.advanceTimersByTimeAsync(400)
    expect(save).toHaveBeenCalledTimes(1)
    // The running save may have been taken before this change.
    saver.touch()
    await vi.advanceTimersByTimeAsync(400)
    expect(save).toHaveBeenCalledTimes(1)
    finish()
    await vi.advanceTimersByTimeAsync(400)
    expect(save).toHaveBeenCalledTimes(2)
  })
})
