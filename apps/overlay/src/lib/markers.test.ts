import { describe, expect, it } from 'vitest'
import type { Pin } from './game'
import { GAME_PING_ICON, LiveMarkerFilter, markerName, withoutAppPings } from './markers'
import type { Marker } from './protocol'

const cart: Pin = { id: 'c', category: 'vehicle', name: 'Golf Cart', icon: 'game:GolfCartIcon', position: { x: 100, y: 10, z: 100 } }
const marker = (id: string, icon: string, x: number, kind: Marker['kind'] = 'locator'): Marker => ({
  id,
  kind,
  label: null,
  icon,
  position: { x, y: 10, z: 100 },
})

describe('LiveMarkerFilter', () => {
  const filter = new LiveMarkerFilter([cart], 1)
  it('hides markers that duplicate a static pin', () => {
    expect(filter.visible([marker('a', 'GolfCartIcon', 102)])).toEqual([])
  })
  it('keeps moved vehicles and markers without a pin', () => {
    const moved = marker('b', 'GolfCartIcon', 300)
    const placed = marker('c', 'GPSTrackerIcon', 100)
    expect(filter.visible([moved, placed])).toEqual([moved, placed])
  })
  it('leaves companions to the player markers', () => {
    expect(filter.visible([marker('k', 'RobbyLogo', 0, 'companion')])).toEqual([])
  })
})

describe('withoutAppPings', () => {
  const ping = (x: number, icon = GAME_PING_ICON): Marker => ({ id: `p${x}`, kind: 'locator', label: 'Ping', icon, position: { x, y: 0, z: 0 } })

  it('drops the copy of a ping the app shows', () => {
    expect(withoutAppPings([ping(0), ping(100)], [{ position: { x: 5, y: 40, z: 0 } }], 1).map((m) => m.id)).toEqual(['p100'])
  })

  it('leaves other markers there alone', () => {
    expect(withoutAppPings([ping(0, 'builtin:bed')], [{ position: { x: 0, y: 0, z: 0 } }], 1)).toHaveLength(1)
  })
})

describe('markerName', () => {
  it('derives a name from the icon', () => {
    expect(markerName(marker('a', 'GolfCartIcon', 0))).toBe('Golf Cart')
    expect(markerName({ ...marker('a', 'RobbyLogo', 0), label: 'Kelvin' })).toBe('Kelvin')
  })
})
