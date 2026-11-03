// Playing together: copies of the app in the same multiplayer session connect (see
// `room.ts`) and share what their players track, map pings and custom pins.

import { useEffect } from 'react'
import { create } from 'zustand'
import { findPin } from '../actions'
import type { Vec3, WorldLayer } from '../protocol'
import { selectAllPins, useStore } from '../../state/store'
import { friendPin, SYNC_VERSION, toSharedPin, type SharedPin, type SharedTarget, type SyncMessage } from './messages'
import { joinSyncRoom, type SyncHandlers, type SyncRoom } from './room'
import { useGameSetting } from '../../settings/store'

export interface Peer {
  id: string
  name: string | null
  /** Said hello on our message version; nothing else from it counts until then. */
  ready: boolean
}

export interface TrackRequest {
  id: number
  from: string
  target: SharedTarget
  at: number
}

export interface Ping {
  id: number
  /** Who pinged; null for your own pings. */
  from: string | null
  position: Vec3
  layer: WorldLayer
  at: number
}

interface SyncState {
  /** The session whose room is joined, if any. */
  session: string | null
  peers: Record<string, Peer>
  peerPins: Record<string, SharedPin[]>
  requests: TrackRequest[]
  pings: Ping[]
}

const EMPTY: SyncState = { session: null, peers: {}, peerPins: {}, requests: [], pings: [] }

export const useSyncStore = create<SyncState>(() => EMPTY)

/** How long pings show, and how long a track request waits for an answer. */
export const PING_MS = 30_000
const REQUEST_MS = 120_000
const MAX_PINGS = 20
const MAX_REQUESTS = 5

/**
 * Debugging without a multiplayer session: a room joined in its place (set
 * `localStorage['game-maps:sync-room']`; see docs/development.md).
 */
function debugRoom(): string | null {
  try {
    return localStorage.getItem('game-maps:sync-room')
  } catch {
    return null
  }
}

let room: SyncRoom | null = null
let seq = 0

/** Players connected and ready (on our message version). */
export function readyPeers(s: Pick<SyncState, 'peers'>): Peer[] {
  return Object.values(s.peers).filter((p) => p.ready)
}

const peerName = (peerId: string) => useSyncStore.getState().peers[peerId]?.name ?? 'A player'

function myName(): string | null {
  return useStore.getState().snapshot?.session?.player_name ?? null
}

function mySharedPins(): SharedPin[] {
  return useStore
    .getState()
    .user.customPins.filter((p) => p.shared)
    .map(toSharedPin)
}

function updateFriendPins() {
  const { peers, peerPins } = useSyncStore.getState()
  const pins = Object.entries(peerPins).flatMap(([peerId, list]) => list.map((p) => friendPin(peerId, peers[peerId]?.name ?? null, p)))
  useStore.getState().setFriendPins(pins)
}

/** Message handlers for a joined room (exported for tests). */
export const handlers: SyncHandlers = {
  onPeerJoin(peerId) {
    useSyncStore.setState((s) => ({ peers: { ...s.peers, [peerId]: { id: peerId, name: null, ready: false } } }))
    room?.send({ type: 'hello', version: SYNC_VERSION, name: myName() }, peerId)
    room?.send({ type: 'pins', pins: mySharedPins() }, peerId)
  },
  onPeerLeave(peerId) {
    useSyncStore.setState((s) => {
      const { [peerId]: _gone, ...peers } = s.peers
      const { [peerId]: _pins, ...peerPins } = s.peerPins
      return { peers, peerPins }
    })
    updateFriendPins()
  },
  onMessage(peerId, message) {
    const peer = useSyncStore.getState().peers[peerId]
    if (message.type === 'hello') {
      const ready = message.version === SYNC_VERSION
      useSyncStore.setState((s) => ({ peers: { ...s.peers, [peerId]: { id: peerId, name: message.name, ready } } }))
      if (ready) useStore.getState().notify(`${message.name ?? 'A player'} is using Game Maps with you`)
      updateFriendPins()
      return
    }
    if (!peer?.ready) return
    const now = Date.now()
    switch (message.type) {
      case 'track':
        useSyncStore.setState((s) => ({
          requests: [...s.requests.filter((r) => r.from !== peerName(peerId)), { id: ++seq, from: peerName(peerId), target: message.target, at: now }].slice(-MAX_REQUESTS),
        }))
        useStore.getState().notify(`${peerName(peerId)} wants you to track ${message.target.name}`)
        break
      case 'ping':
        addPing({ id: ++seq, from: peerName(peerId), position: message.position, layer: message.layer, at: now })
        useStore.getState().notify(`${peerName(peerId)} pinged the map`)
        break
      case 'pins':
        useSyncStore.setState((s) => ({ peerPins: { ...s.peerPins, [peerId]: message.pins } }))
        updateFriendPins()
        break
    }
  },
}

function addPing(ping: Ping) {
  useSyncStore.setState((s) => ({ pings: [...s.pings, ping].slice(-MAX_PINGS) }))
}

/** Starts using `next` as the room (or none); exported for tests. */
export function attachRoom(next: SyncRoom | null, session: string | null) {
  room?.leave()
  room = next
  useSyncStore.setState({ ...EMPTY, session })
  updateFriendPins()
}

function send(message: SyncMessage): number {
  const peers = readyPeers(useSyncStore.getState())
  if (!room || peers.length === 0) return 0
  room.send(message)
  return peers.length
}

/** Asks everyone in the session to track `target` too. */
export function shareTrack(target: SharedTarget) {
  const sent = send({ type: 'track', target })
  useStore.getState().notify(sent ? `Asked ${sent === 1 ? '1 player' : `${sent} players`} to track ${target.name}` : 'Nobody else is connected', sent ? 'info' : 'error')
}

/** Pings a spot on everyone's map (yours included). */
export function sendPing(position: Vec3, layer: WorldLayer) {
  addPing({ id: ++seq, from: null, position, layer, at: Date.now() })
  send({ type: 'ping', position, layer })
}

/** Tracks what a request asked for: the map pin when this map has it, else the spot. */
export function acceptRequest(id: number) {
  const request = useSyncStore.getState().requests.find((r) => r.id === id)
  if (!request) return
  const { target } = request
  const pin = target.pinId ? findPin(target.pinId) : undefined
  useStore.getState().setTracked(pin ? { kind: 'pin', id: pin.id } : { kind: 'point', name: target.name, position: target.position, layer: target.layer })
  dismissRequest(id)
}

export function dismissRequest(id: number) {
  useSyncStore.setState((s) => ({ requests: s.requests.filter((r) => r.id !== id) }))
}

/** Drops pings and requests that have run their time. */
export function expire(now = Date.now()) {
  const s = useSyncStore.getState()
  const pings = s.pings.filter((p) => now - p.at < PING_MS)
  const requests = s.requests.filter((r) => now - r.at < REQUEST_MS)
  if (pings.length !== s.pings.length || requests.length !== s.requests.length) useSyncStore.setState({ pings, requests })
}

/**
 * Keeps the room joined while playing together is on and the game reports a
 * multiplayer session, and tells the others when shared pins change.
 */
export function useSync() {
  const gameId = useStore((s) => s.game?.id ?? null)
  const enabled = useGameSetting(gameId, 'playTogether')
  const session = useStore((s) => s.snapshot?.session?.id ?? null) ?? debugRoom()
  useEffect(() => {
    if (!enabled || !gameId || !session) return
    attachRoom(joinSyncRoom(gameId, session, handlers), session)
    const unsubscribe = useStore.subscribe((s, prev) => {
      if (s.user.customPins !== prev.user.customPins) send({ type: 'pins', pins: mySharedPins() })
    })
    const timer = setInterval(() => expire(), 1000)
    return () => {
      unsubscribe()
      clearInterval(timer)
      attachRoom(null, null)
    }
  }, [enabled, gameId, session])
}

/** A pin's details as something to track together. */
export function targetOfPin(pinId: string): SharedTarget | null {
  const pin = selectAllPins(useStore.getState()).find((p) => p.id === pinId)
  return pin ? { name: pin.name, position: pin.position, layer: pin.layer ?? 'surface', pinId: pin.id } : null
}
