import { beforeEach, describe, expect, it } from 'vitest'
import { withGameSettings } from '../../settings/model'
import { useSettings } from '../../settings/store'
import { EMPTY_USER_DATA, useStore } from '../../state/store'
import type { GameDefinition, Pin } from '../game'
import type { SyncMessage } from './messages'
import type { SyncRoom } from './room'
import { acceptLatestRequest, acceptRequest, attachRoom, expire, handlers, hasSyncNotice, NOTICE_MS, PING_MS, sendPing, shareTrack, useSyncStore } from './sync'

const at = { x: 1, y: 2, z: 3 }

/** A room that records what it sends. */
function fakeRoom() {
  const sent: { message: SyncMessage; to?: string }[] = []
  const room: SyncRoom = { send: (message, to) => void sent.push({ message, to }), leave: async () => {}, probe: async () => ({ route: null, ms: null }) }
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
    useSyncStore.setState({ requests: [], pings: [], notices: [], report: null })
    attachRoom(fake.room, 'lobby:1')
    useStore.setState({ tracked: null, friendPins: [], user: EMPTY_USER_DATA })
  })

  it('greets peers and sends them the shared pins', () => {
    useStore.setState({
      user: {
        ...EMPTY_USER_DATA,
        customPins: [
          { id: 'c1', category: 'custom', name: 'Base', position: at, custom: true, shared: true },
          { id: 'c2', category: 'custom', name: 'Secret', position: at, custom: true },
        ],
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

  it('keeps a peer ready when its hello arrives before the join event', () => {
    handlers.onMessage('p1', { type: 'hello', version: 1, name: 'Ana' })
    handlers.onPeerJoin('p1')
    expect(useSyncStore.getState().peers.p1).toMatchObject({ name: 'Ana', ready: true })
  })

  it('tells the player who connected, for showing over the game', () => {
    ready('p1', 'Ana')
    expect(useSyncStore.getState().notices.map((n) => n.text)).toEqual(['Ana is using Game Maps with you'])
  })

  it('tells the player about pins friends newly share, once', () => {
    const base = { id: 'c1', name: 'Base', position: at, layer: 'surface', category: 'custom' } as const
    const mine = { ...base, id: 'c2', name: 'Mine' }
    ready('p1', 'Ana')
    useSyncStore.setState({ notices: [] })
    handlers.onMessage('p1', { type: 'pins', pins: [] })
    expect(useSyncStore.getState().notices).toEqual([])
    handlers.onMessage('p1', { type: 'pins', pins: [base] })
    handlers.onMessage('p1', { type: 'pins', pins: [base] })
    expect(useSyncStore.getState().notices.map((n) => n.text)).toEqual(['Ana shared Base'])
    useSyncStore.setState({ notices: [] })
    handlers.onMessage('p1', { type: 'pins', pins: [base, mine, { ...mine, id: 'c3' }] })
    expect(useSyncStore.getState().notices.map((n) => n.text)).toEqual(['Ana shared 2 pins'])
  })

  it('leaves found items alone unless sharing progress', () => {
    ready('p1', 'Ana')
    handlers.onMessage('p1', { type: 'found', ids: ['a'] })
    expect(useStore.getState().user.found).toEqual([])
  })

  it('adds friends’ found items when sharing progress', () => {
    useStore.setState({ game: { id: 'g' } as GameDefinition, gamePins: [{ id: 'a' }, { id: 'b' }] as Pin[] })
    useSettings.setState({ ui: withGameSettings(useSettings.getState().ui, 'g', { shareProgress: true }) })
    ready('p1', 'Ana')
    handlers.onMessage('p1', { type: 'found', ids: ['a', 'b', 'not-on-this-map'] })
    expect(useStore.getState().user.found).toEqual(['a', 'b'])
    useStore.setState({ gamePins: [] })
    useSettings.setState({ ui: withGameSettings(useSettings.getState().ui, 'g', { shareProgress: false }) })
  })

  it('sends what was found to a friend who connects, when sharing progress', () => {
    useStore.setState({ game: { id: 'g' } as GameDefinition, user: { ...EMPTY_USER_DATA, found: ['a'] } })
    useSettings.setState({ ui: withGameSettings(useSettings.getState().ui, 'g', { shareProgress: true }) })
    ready('p1', 'Ana')
    expect(sent.at(-1)).toEqual({ message: { type: 'found', ids: ['a'] }, to: 'p1' })
    useSettings.setState({ ui: withGameSettings(useSettings.getState().ui, 'g', { shareProgress: false }) })
  })

  it('accepts the newest request with the accept key', () => {
    ready('p1', 'Ana')
    expect(acceptLatestRequest()).toBe(false)
    handlers.onMessage('p1', { type: 'track', target: { name: 'Shovel', position: at, layer: 'surface' } })
    expect(acceptLatestRequest()).toBe(true)
    expect(useStore.getState().tracked).toMatchObject({ kind: 'point', name: 'Shovel' })
    expect(useSyncStore.getState().requests).toEqual([])
  })

  it('lets notices fade but keeps a track request waiting', () => {
    ready('p1', 'Ana')
    handlers.onMessage('p1', { type: 'track', target: { name: 'Shovel', position: at, layer: 'surface' } })
    expire(Date.now() + NOTICE_MS + 1)
    expect(useSyncStore.getState().notices).toEqual([])
    expect(useSyncStore.getState().requests).toHaveLength(1)
  })
})

describe('hasSyncNotice', () => {
  const target = { name: 'Shovel', position: at, layer: 'surface' } as const

  it('is true while a track request or notice waits to be seen', () => {
    expect(hasSyncNotice({ requests: [], notices: [] })).toBe(false)
    expect(hasSyncNotice({ requests: [{ id: 1, peerId: 'a', from: 'Ana', target, at: 0 }], notices: [] })).toBe(true)
    expect(hasSyncNotice({ requests: [], notices: [{ id: 1, text: 'Hi', at: 0 }] })).toBe(true)
  })
})
