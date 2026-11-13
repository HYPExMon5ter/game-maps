import { describe, expect, it } from 'vitest'
import type { PackStatus, RemotePack } from '../lib/backend'
import { download3d } from './view3d'

const remote = (size: number, extras?: Record<string, RemotePack>): RemotePack => ({ version: '1', url: '', sha256: '', size, extras })

function pack(p: Partial<PackStatus>): PackStatus {
  return { game_id: 'sotf', installed: '1', available: remote(30, { '3d': remote(11_000_000) }), downloading: null, error: null, extras: {}, ...p }
}

describe('download3d', () => {
  it('offers the 3D map when it is published and not here', () => {
    expect(download3d(pack({}))).toEqual({ state: 'available', size: 11_000_000, error: null })
  })

  it('reports progress while downloading', () => {
    const p = pack({ extras: { '3d': { installed: null, downloading: { received: 25, total: 100 }, error: null } } })
    expect(download3d(p)).toEqual({ state: 'downloading', progress: 0.25, total: 100 })
  })

  it('is installed once downloaded, or from the repository in development', () => {
    expect(download3d(pack({ extras: { '3d': { installed: '1', downloading: null, error: null } } })).state).toBe('installed')
    expect(download3d(pack({ available: null, extras: { '3d': { installed: 'development', downloading: null, error: null } } })).state).toBe('installed')
  })

  it('keeps a failed download on offer, with its error', () => {
    const p = pack({ extras: { '3d': { installed: null, downloading: null, error: 'download failed' } } })
    expect(download3d(p)).toEqual({ state: 'available', size: 11_000_000, error: 'download failed' })
  })

  it('has nothing to offer without a published 3D map', () => {
    expect(download3d(pack({ available: remote(30) })).state).toBe('none')
    expect(download3d(undefined).state).toBe('none')
  })
})
