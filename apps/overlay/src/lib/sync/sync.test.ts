import { beforeEach, describe, expect, it } from 'vitest'
import { useStore } from '../../state/store'
import type { SyncMessage } from './messages'
import type { SyncRoom } from './room'
import { acceptRequest, attachRoom, expire, handlers, PING_MS, sendPing, shareTrack, useSyncStore } from './sync'

const at = { x: 1, y: 2, z: 3 }

/** A room that records what it sends. */
function fakeRoom() {
  const sent: { message: SyncMessage; to?: string }[] = []
  const room: SyncRoom = { send: (message, to) => void sent.push({ message, to }), leave: () => {} }
  return { room, sent }
}

function ready(peerId: string, name: string) {
  handlers.onPeerJoin(peerId)
  handlers.onMessage(peerId, { type: 'hello', version: 1, name })
}

describe('sync', () => {
  let sent: ReturnType<typeof fakeRoom>['sent']
  beforeEach(() => {
    const fake = fakeRoom()
    sent = fake.sent
    attachRoom(fake.room, 'lobby:1')
    useStore.setState({ tracked: null, friendPins: [], user: { customPins: [], found: [], hiddenCategories: [], shownCategories: [] } })
  })

  it('greets peers and sends them the shared pins', () => {
    useStore.setState({
      user: {
        customPins: [
          { id: 'c1', category: 'custom', name: 'Base', position: at, custom: true, shared: true },
          { id: 'c2', category: 'custom', name: 'Secret', position: at, custom: true },
        ],
        found: [],
        hiddenCategories: [],
        shownCategories: [],
      },
    })
    handlers.onPeerJoin('p1')
    expect(sent.map((s) => [s.message.type, s.to])).toEqual([
      ['hello', 'p1'],
      ['pins', 'p1'],
    ])
    const pins = sent[1].message.type === 'pins' ? sent[1].message.pins : []
    expect(pins.map((p) => p.name)).toEqual(['Base'])
  })

  it('ignores peers until they say hello on the same version', () => {
    handlers.onPeerJoin('p1')
    handlers.onMessage('p1', { type: 'ping', position: at, layer: 'surface' })
    expect(useSyncStore.getState().pings).toEqual([])
    handlers.onMessage('p1', { type: 'hello', version: 99, name: 'Future' })
    handlers.onMessage('p1', { type: 'ping', position: at, layer: 'surface' })
    expect(useSyncStore.getState().pings).toEqual([])
  })

  it('shows friends’ pins while they are connected', () => {
    ready('p1', 'Ana')
    handlers.onMessage('p1', { type: 'pins', pins: [{ id: 'c1', name: 'Base', position: at, layer: 'surface', category: 'custom' }] })
    expect(useStore.getState().friendPins.map((p) => p.id)).toEqual(['friend:p1:c1'])
    handlers.onPeerLeave('p1')
    expect(useStore.getState().friendPins).toEqual([])
  })

  it('turns an accepted track request into tracking', () => {
    ready('p1', 'Ana')
    handlers.onMessage('p1', { type: 'track', target: { name: 'Shovel', position: at, layer: 'caves', pinId: 'not-on-this-map' } })
    const [request] = useSyncStore.getState().requests
    expect(request.from).toBe('Ana')
    acceptRequest(request.id)
    expect(useStore.getState().tracked).toEqual({ kind: 'point', name: 'Shovel', position: at, layer: 'caves' })
    expect(useSyncStore.getState().requests).toEqual([])
  })

  it('sends pings and track requests only when someone is there', () => {
    shareTrack({ name: 'Shovel', position: at, layer: 'surface' })
    expect(sent).toEqual([])
    ready('p1', 'Ana')
    sent.length = 0
    shareTrack({ name: 'Shovel', position: at, layer: 'surface' })
    sendPing(at, 'surface')
    expect(sent.map((s) => s.message.type)).toEqual(['track', 'ping'])
    expect(useSyncStore.getState().pings.map((p) => p.from)).toEqual([null])
  })

  it('lets pings fade after a while', () => {
    sendPing(at, 'surface')
    expire(Date.now() + PING_MS + 1)
    expect(useSyncStore.getState().pings).toEqual([])
  })
})
