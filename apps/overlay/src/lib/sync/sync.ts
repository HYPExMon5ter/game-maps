// Playing together: copies of the app in the same multiplayer session connect (see
// `room.ts`) and share what their players track, map pings, custom pins and, if they
// choose, their progress. The overlay runs the connection; the map window mirrors its
// state and sends its actions over (see `useSyncMirror`).

import { emit, listen } from '@tauri-apps/api/event'
import { useEffect, useRef, useState } from 'react'
import { create } from 'zustand'
import { findPin } from '../actions'
import { backend } from '../backend'
import { logUiError, logUiInfo } from '../errors'
import type { Pin } from '../game'
import type { Vec3, WorldLayer } from '../protocol'
import { IS_MAP_WINDOW } from '../view'
import { gameSettings } from '../../settings/model'
import { useGameSetting, useSettings } from '../../settings/store'
import { currentFog, mergeFriendFog } from '../../state/fog'
import { selectAllPins, useStore } from '../../state/store'
import { classifyFailure, probeNetwork, probeRelays, type ConnectionReport, type PeerFailure, type PeerProbe } from './connectionCheck'
import { friendPin, MAX_PINS, SYNC_VERSION, toSharedPin, type SharedPin, type SharedTarget, type SyncMessage } from './messages'
import { rejoinDelay, shouldRejoin } from './rejoin'
import { joinSyncRoom, type SyncHandlers, type SyncRoom } from './room'

export interface Peer {
  id: string
  name: string | null
  /** Said hello on our message version; nothing else from it counts until then. */
  ready: boolean
}

export interface TrackRequest {
  id: number
  /** Who asked: names aren't unique, or known yet. */
  peerId: string
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

/** Something to tell the player, shown briefly over the game. */
export interface Notice {
  id: number
  text: string
  at: number
}

interface SyncState {
  /** The session whose room is joined, if any. */
  session: string | null
  peers: Record<string, Peer>
  peerPins: Record<string, SharedPin[]>
  requests: TrackRequest[]
  pings: Ping[]
  notices: Notice[]
  /** Peers found but not linked with (latest attempt each). */
  failures: PeerFailure[]
  checking: boolean
  /** The last connection check. */
  report: ConnectionReport | null
}

const EMPTY: SyncState = { session: null, peers: {}, peerPins: {}, requests: [], pings: [], notices: [], failures: [], checking: false, report: null }

export const useSyncStore = create<SyncState>(() => EMPTY)

/** How long pings and notices show, and how long a track request waits for an answer. */
export const PING_MS = 30_000
export const NOTICE_MS = 6_000
const REQUEST_MS = 120_000
const MAX_PINGS = 20
const MAX_REQUESTS = 5
const MAX_NOTICES = 3
const MAX_FAILURES = 10
/** How often explored areas go out while sharing progress. */
const FOG_SHARE_MS = 30_000
/** How often to check whether the room has ended up connected to nobody. */
const ROOM_WATCH_MS = 5000

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
/** The room being left: joining it again has to wait. */
let leaving: Promise<void> = Promise.resolve()
let seq = 0

/** Players connected and ready (on our message version). */
export function readyPeers(s: Pick<SyncState, 'peers'>): Peer[] {
  return Object.values(s.peers).filter((p) => p.ready)
}

/** Whether there's something to put in front of the player while they play. */
export function hasSyncNotice(s: Pick<SyncState, 'requests' | 'notices'>): boolean {
  return s.requests.length > 0 || s.notices.length > 0
}

const peerName = (peerId: string) => useSyncStore.getState().peers[peerId]?.name ?? 'A player'

function myName(): string | null {
  return useStore.getState().snapshot?.session?.player_name ?? null
}

function mySharedPins(): SharedPin[] {
  return useStore
    .getState()
    .user.customPins.filter((p) => p.shared)
    .slice(0, MAX_PINS)
    .map(toSharedPin)
}

function updateFriendPins() {
  const { peers, peerPins } = useSyncStore.getState()
  const pins = Object.entries(peerPins).flatMap(([peerId, list]) => list.map((p) => friendPin(peerId, peers[peerId]?.name ?? null, p)))
  useStore.getState().setFriendPins(pins)
}

function sharingProgress(): boolean {
  return gameSettings(useSettings.getState().ui, useStore.getState().game?.id).shareProgress
}

/** Tells `peerId` (or everyone) what the player has found and explored, when sharing. */
function sendProgress(peerId?: string) {
  if (!sharingProgress()) return
  const { found } = useStore.getState().user
  if (found.length) room?.send({ type: 'found', ids: found }, peerId)
  const fog = currentFog()
  if (fog) room?.send({ type: 'fog', fog }, peerId)
}

/** Tells the player something, whether they're on the map or in the game. */
export function announce(text: string) {
  useStore.getState().notify(text)
  useSyncStore.setState((s) => ({ notices: [...s.notices, { id: ++seq, text, at: Date.now() }].slice(-MAX_NOTICES) }))
}

/** Message handlers for a joined room (exported for tests). */
export const handlers: SyncHandlers = {
  onPeerJoin(peerId) {
    logUiInfo(`play together: connected to peer ${peerId}`)
    void backend.countFeature('multiplayer').catch(() => {})
    // Its hello can get here first; don't forget it.
    useSyncStore.setState((s) => ({
      peers: { ...s.peers, [peerId]: s.peers[peerId] ?? { id: peerId, name: null, ready: false } },
      failures: s.failures.filter((f) => f.peerId !== peerId),
    }))
    room?.send({ type: 'hello', version: SYNC_VERSION, name: myName() }, peerId)
    room?.send({ type: 'pins', pins: mySharedPins() }, peerId)
  },
  onPeerLeave(peerId) {
    logUiInfo(`play together: peer ${peerId} left`)
    useSyncStore.setState((s) => {
      const { [peerId]: _gone, ...peers } = s.peers
      const { [peerId]: _pins, ...peerPins } = s.peerPins
      return { peers, peerPins }
    })
    updateFriendPins()
  },
  onPeerFailed(peerId, error) {
    const failure = { peerId, kind: classifyFailure(error) }
    useSyncStore.setState((s) => ({ failures: [...s.failures.filter((f) => f.peerId !== peerId), failure].slice(-MAX_FAILURES) }))
  },
  onMessage(peerId, message) {
    const peer = useSyncStore.getState().peers[peerId]
    if (message.type === 'hello') {
      const ready = message.version === SYNC_VERSION
      logUiInfo(`play together: peer ${peerId} is ${message.name ?? 'unnamed'} on message version ${message.version} (ours ${SYNC_VERSION})`)
      useSyncStore.setState((s) => ({ peers: { ...s.peers, [peerId]: { id: peerId, name: message.name, ready } } }))
      updateFriendPins()
      // Only the first: a repeated hello doesn't need another round of everything.
      if (ready && !peer?.ready) {
        announce(`${message.name ?? 'A player'} is using Game Maps with you`)
        sendProgress(peerId)
      }
      return
    }
    if (!peer?.ready) return
    const now = Date.now()
    switch (message.type) {
      case 'track':
        logUiInfo(`play together: ${peerName(peerId)} asked to track ${message.target.name}`)
        useSyncStore.setState((s) => ({
          requests: [...s.requests.filter((r) => r.peerId !== peerId), { id: ++seq, peerId, from: peerName(peerId), target: message.target, at: now }].slice(-MAX_REQUESTS),
        }))
        useStore.getState().notify(`${peerName(peerId)} wants you to track ${message.target.name}`)
        break
      case 'ping':
        addPing({ id: ++seq, from: peerName(peerId), position: message.position, layer: message.layer, at: now })
        useStore.getState().notify(`${peerName(peerId)} pinged the map`)
        break
      case 'pins': {
        const known = new Set(useSyncStore.getState().peerPins[peerId]?.map((p) => p.id))
        const added = message.pins.filter((p) => !known.has(p.id))
        useSyncStore.setState((s) => ({ peerPins: { ...s.peerPins, [peerId]: message.pins } }))
        updateFriendPins()
        if (added.length > 0) announce(`${peerName(peerId)} shared ${added.length === 1 ? added[0].name : `${added.length} pins`}`)
        break
      }
      case 'found': {
        if (!sharingProgress()) break
        // Only this map's pins: what's kept is saved, and it comes from someone else.
        const known = new Set(useStore.getState().gamePins.map((p) => p.id))
        useStore.getState().markFound(message.ids.filter((id) => known.has(id)))
        break
      }
      case 'fog':
        if (sharingProgress()) mergeFriendFog(message.fog)
        break
    }
  },
}

function addPing(ping: Ping) {
  useSyncStore.setState((s) => ({ pings: [...s.pings, ping].slice(-MAX_PINGS) }))
}

/** Starts using `next` as the room (or none); exported for tests. */
export function attachRoom(next: SyncRoom | null, session: string | null) {
  if (room) leaving = room.leave()
  room = next
  // What's on screen stays through a reconnect, and runs out by itself.
  useSyncStore.setState((s) => ({ ...EMPTY, session, requests: s.requests, pings: s.pings, notices: s.notices, report: s.report }))
  updateFriendPins()
}

function send(message: SyncMessage): number {
  const peers = readyPeers(useSyncStore.getState())
  if (!room || peers.length === 0) return 0
  room.send(message)
  return peers.length
}

/** What the map window asks the overlay to do. */
type SyncAction = { kind: 'track'; target: SharedTarget } | { kind: 'ping'; position: Vec3; layer: WorldLayer } | { kind: 'accept'; id: number } | { kind: 'dismiss'; id: number } | { kind: 'check' }

const STATE_EVENT = 'sync-state'
const STATE_WANTED_EVENT = 'sync-state-wanted'
const ACTION_EVENT = 'sync-action'

/** In the map window: hands `action` to the overlay. */
function forwarded(action: SyncAction): boolean {
  if (IS_MAP_WINDOW) void emit(ACTION_EVENT, action)
  return IS_MAP_WINDOW
}

/** Asks everyone in the session to track `target` too. */
export function shareTrack(target: SharedTarget) {
  if (forwarded({ kind: 'track', target })) return
  const sent = send({ type: 'track', target })
  logUiInfo(`play together: asked ${sent} player(s) to track ${target.name}`)
  useStore.getState().notify(sent ? `Asked ${sent === 1 ? '1 player' : `${sent} players`} to track ${target.name}` : 'Nobody else is connected. See the connection check in Settings, Multiplayer', sent ? 'info' : 'error')
}

/** Measures what's between the player and their friends (see `connectionCheck`). */
export async function runConnectionCheck() {
  if (forwarded({ kind: 'check' }) || useSyncStore.getState().checking) return
  useSyncStore.setState({ checking: true })
  const probePeers = Object.keys(useSyncStore.getState().peers).map(async (id): Promise<[string, PeerProbe]> => [id, (await room?.probe(id)) ?? { route: null, ms: null }])
  const [relays, network, peers] = await Promise.all([probeRelays(), probeNetwork().catch(() => 'blocked' as const), Promise.all(probePeers)])
  const report: ConnectionReport = { relays, network, peers: Object.fromEntries(peers) }
  logUiInfo(`play together: check: ${relays.reached}/${relays.tried} servers, network ${network}, ${peers.length} peer(s) ${JSON.stringify(report.peers)}, ${useSyncStore.getState().failures.length} failed`)
  useSyncStore.setState({ checking: false, report })
}

/** Pings a spot on everyone's map (yours included). */
export function sendPing(position: Vec3, layer: WorldLayer) {
  if (forwarded({ kind: 'ping', position, layer })) return
  addPing({ id: ++seq, from: null, position, layer, at: Date.now() })
  send({ type: 'ping', position, layer })
}

/** Tracks what a request asked for: the map pin when this map has it, else the spot. */
export function acceptRequest(id: number) {
  if (forwarded({ kind: 'accept', id })) return
  const request = useSyncStore.getState().requests.find((r) => r.id === id)
  if (!request) return
  const { target } = request
  const pin = target.pinId ? findPin(target.pinId) : undefined
  useStore.getState().setTracked(pin ? { kind: 'pin', id: pin.id } : { kind: 'point', name: target.name, position: target.position, layer: target.layer })
  dismissRequest(id)
}

export function dismissRequest(id: number) {
  if (forwarded({ kind: 'dismiss', id })) return
  useSyncStore.setState((s) => ({ requests: s.requests.filter((r) => r.id !== id) }))
}

/** Accepts the newest request (the accept key); false when none is waiting. */
export function acceptLatestRequest(): boolean {
  const latest = useSyncStore.getState().requests.at(-1)
  if (!latest) return false
  acceptRequest(latest.id)
  announce(`Tracking ${latest.target.name}`)
  return true
}

/** Drops pings, requests and notices that have run their time. */
export function expire(now = Date.now()) {
  const s = useSyncStore.getState()
  const pings = s.pings.filter((p) => now - p.at < PING_MS)
  const requests = s.requests.filter((r) => now - r.at < REQUEST_MS)
  const notices = s.notices.filter((n) => now - n.at < NOTICE_MS)
  if (pings.length !== s.pings.length || requests.length !== s.requests.length || notices.length !== s.notices.length) useSyncStore.setState({ pings, requests, notices })
}

/**
 * Keeps the room joined while playing together is on and the game reports a
 * multiplayer session, and tells the others when shared pins change.
 */
export function useSync() {
  const gameId = useStore((s) => s.game?.id ?? null)
  const enabled = useGameSetting(gameId, 'playTogether')
  const session = useStore((s) => s.snapshot?.session?.id ?? null) ?? debugRoom()
  // Bumped to leave the room and join it again.
  const [attempt, setAttempt] = useState(0)
  const rejoins = useRef(0)
  useEffect(() => {
    rejoins.current = 0
  }, [session])
  useEffect(() => {
    if (!enabled || !gameId || !session) return
    let active = true
    let retry: ReturnType<typeof setTimeout> | undefined
    const joinedAt = Date.now()
    const rejoin = () => setAttempt((a) => a + 1)
    void leaving.then(() => {
      if (!active) return
      // Never the session id: it's the room's password, and logs get shared.
      logUiInfo(`play together: joining the ${gameId} session`)
      try {
        attachRoom(joinSyncRoom(gameId, session, handlers), session)
      } catch (e) {
        logUiError(`play together: could not join: ${String(e)}`)
        retry = setTimeout(rejoin, rejoinDelay(rejoins.current++))
      }
    })
    // Connected to nobody with friends around: a link failed or they were missed.
    const watchdog = setInterval(() => {
      const s = useSyncStore.getState()
      const peers = Object.keys(s.peers).length
      if (peers > 0) rejoins.current = 0
      const players = useStore.getState().snapshot?.others.length ?? 0
      if (!shouldRejoin({ peers, players, failures: s.failures.length, waitedMs: Date.now() - joinedAt, attempts: rejoins.current })) return
      rejoins.current++
      logUiInfo(`play together: nobody connected (${players} in the game, ${s.failures.length} failed); joining again (attempt ${rejoins.current})`)
      rejoin()
    }, ROOM_WATCH_MS)
    const onOnline = () => {
      if (Object.keys(useSyncStore.getState().peers).length > 0) return
      logUiInfo('play together: back online; joining again')
      rejoin()
    }
    window.addEventListener('online', onOnline)
    const unsubscribe = useStore.subscribe((s, prev) => {
      if (s.user.customPins !== prev.user.customPins) send({ type: 'pins', pins: mySharedPins() })
      // Not a world change: those are that world's finds, not new ones.
      if (s.user.found !== prev.user.found && s.world.id === prev.world.id && sharingProgress()) {
        const before = new Set(prev.user.found)
        const ids = s.user.found.filter((id) => !before.has(id))
        if (ids.length) send({ type: 'found', ids })
      }
    })
    let sentFog: string | null = null
    const fogTimer = setInterval(() => {
      const fog = sharingProgress() ? currentFog() : null
      if (!fog || fog.bits === sentFog) return
      if (send({ type: 'fog', fog })) sentFog = fog.bits
    }, FOG_SHARE_MS)
    return () => {
      active = false
      clearTimeout(retry)
      clearInterval(watchdog)
      window.removeEventListener('online', onOnline)
      unsubscribe()
      clearInterval(fogTimer)
      logUiInfo('play together: left the room')
      attachRoom(null, null)
    }
  }, [enabled, gameId, session, attempt])

  // Not tied to a room: notices show in single player too.
  useEffect(() => {
    const timer = setInterval(() => expire(), 1000)
    return () => clearInterval(timer)
  }, [])

  useEffect(() => {
    const publish = () => void emit(STATE_EVENT, { sync: useSyncStore.getState(), friendPins: useStore.getState().friendPins })
    const stop = useSyncStore.subscribe(publish)
    const stopPins = useStore.subscribe((s, prev) => {
      if (s.friendPins !== prev.friendPins) publish()
    })
    const actions = listen<SyncAction>(ACTION_EVENT, ({ payload: a }) => {
      if (a.kind === 'track') shareTrack(a.target)
      else if (a.kind === 'ping') sendPing(a.position, a.layer)
      else if (a.kind === 'accept') acceptRequest(a.id)
      else if (a.kind === 'check') void runConnectionCheck()
      else dismissRequest(a.id)
    })
    const wanted = listen(STATE_WANTED_EVENT, publish)
    return () => {
      stop()
      stopPins()
      void actions.then((f) => f())
      void wanted.then((f) => f())
    }
  }, [])
}

/** The map window: shows what the overlay's connection knows. */
export function useSyncMirror() {
  useEffect(() => {
    const state = listen<{ sync: SyncState; friendPins: Pin[] }>(STATE_EVENT, ({ payload }) => {
      useSyncStore.setState(payload.sync)
      useStore.getState().setFriendPins(payload.friendPins)
    })
    void state.then(() => emit(STATE_WANTED_EVENT))
    return () => void state.then((f) => f())
  }, [])
}

/** A pin's details as something to track together. */
export function targetOfPin(pinId: string): SharedTarget | null {
  const pin = selectAllPins(useStore.getState()).find((p) => p.id === pinId)
  return pin ? { name: pin.name, position: pin.position, layer: pin.layer ?? 'surface', pinId: pin.id } : null
}
