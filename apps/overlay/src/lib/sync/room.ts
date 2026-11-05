// Peer-to-peer connection between copies of the app in the same multiplayer session.
// No server of our own: peers find each other through public Nostr relays (Trystero),
// then talk directly over WebRTC. The room id and password come from the game session,
// which only players in it know, so the relays see an opaque topic and encrypted
// connection offers, and nobody outside the session can join.

import { joinRoom, type Room } from 'trystero'
import { logUiError } from '../errors'
import { routeFromStats, type PeerProbe } from './connectionCheck'
import { parseMessage, type SyncMessage } from './messages'

const APP_ID = 'game-maps-overlay'
/** How long a peer gets to answer a ping during the connection check. */
const PROBE_TIMEOUT_MS = 3000

export interface SyncHandlers {
  onPeerJoin(peerId: string): void
  onPeerLeave(peerId: string): void
  onMessage(peerId: string, message: SyncMessage): void
  /** Found a peer but couldn't link with it. */
  onPeerFailed(peerId: string, error: string): void
}

export interface SyncRoom {
  /** To one peer, or everyone when `peerId` is omitted. */
  send(message: SyncMessage, peerId?: string): void
  /** Done once the room can be joined again. */
  leave(): Promise<void>
  /** How the connection to a peer runs right now. */
  probe(peerId: string): Promise<PeerProbe>
}

async function measure(room: Room, peerId: string): Promise<PeerProbe> {
  const timeout = new Promise<null>((resolve) => setTimeout(() => resolve(null), PROBE_TIMEOUT_MS))
  const [ms, stats] = await Promise.all([
    Promise.race([room.ping(peerId), timeout]).catch(() => null),
    room.getPeers()[peerId]?.getStats().catch(() => null),
  ])
  return { ms, route: stats ? routeFromStats(stats.values()) : null }
}

/** Joins the room for `gameId`'s multiplayer `session`. */
export function joinSyncRoom(gameId: string, session: string, handlers: SyncHandlers): SyncRoom {
  const room: Room = joinRoom({ appId: APP_ID, password: session }, `${gameId}/${session}`, {
    onJoinError: ({ error, peerId }) => {
      logUiError(`play together: peer ${peerId} could not join: ${error}`)
      handlers.onPeerFailed(peerId, error)
    },
  })
  const action = room.makeAction<string>('msg')
  action.onMessage = (data, { peerId }) => {
    let message: SyncMessage | null = null
    try {
      message = parseMessage(JSON.parse(data))
    } catch {
      // Not JSON: ignore.
    }
    if (!message) return
    try {
      handlers.onMessage(peerId, message)
    } catch (e) {
      // What a peer sent can't be trusted to be usable; it's no reason to stop listening.
      logUiError(`play together: could not handle ${message.type}: ${String(e)}`)
    }
  }
  room.onPeerJoin = handlers.onPeerJoin
  room.onPeerLeave = handlers.onPeerLeave
  return {
    send: (message, peerId) => void action.send(JSON.stringify(message), peerId ? { target: peerId } : undefined).catch((e: unknown) => logUiError(`play together: could not send ${message.type}: ${String(e)}`)),
    leave: () => room.leave().catch((e: unknown) => logUiError(`play together: could not leave the room: ${String(e)}`)),
    probe: (peerId) => measure(room, peerId),
  }
}
