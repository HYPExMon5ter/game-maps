import { describe, expect, it } from 'vitest'
import { toastMs } from './toast'

describe('toastMs', () => {
  it('keeps short notices brief', () => {
    expect(toastMs('Coordinates copied', 'info')).toBe(2600)
  })

  it('gives longer text longer', () => {
    const long = 'Found: Printer Arm, Golf Cart Keys, Maintenance Keycard and Guest Keycard'
    expect(toastMs(long, 'info')).toBeGreaterThan(5000)
    expect(toastMs(long, 'info')).toBeGreaterThan(toastMs('Found: Rope', 'info'))
  })

  it('gives errors time to be read', () => {
    expect(toastMs('Teleport failed', 'error')).toBe(5000)
    expect(toastMs('Could not save your map data. It stays here while the app is open', 'error')).toBeGreaterThan(5000)
  })

  it('never stays up for good', () => {
    expect(toastMs('x'.repeat(2000), 'error')).toBe(10_000)
  })
})
