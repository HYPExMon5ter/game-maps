// Peer-to-peer connection between copies of the app in the same multiplayer session.
// No server of our own: peers find each other through public Nostr relays (Trystero),
// then talk directly over WebRTC. The room id and password come from the game session,
// which only players in it know, so the relays see an opaque topic and encrypted
// connection offers, and nobody outside the session can join.

import { joinRoom, type Room } from 'trystero'
import { parseMessage, type SyncMessage } from './messages'

const APP_ID = 'game-maps-overlay'

export interface SyncHandlers {
  onPeerJoin(peerId: string): void
  onPeerLeave(peerId: string): void
  onMessage(peerId: string, message: SyncMessage): void
}

export interface SyncRoom {
  /** To one peer, or everyone when `peerId` is omitted. */
  send(message: SyncMessage, peerId?: string): void
  leave(): void
}

/** Joins the room for `gameId`'s multiplayer `session`. */
export function joinSyncRoom(gameId: string, session: string, handlers: SyncHandlers): SyncRoom {
  const room: Room = joinRoom({ appId: APP_ID, password: session }, `${gameId}/${session}`)
  const action = room.makeAction<string>('msg')
  action.onMessage = (data, { peerId }) => {
    let message: SyncMessage | null = null
    try {
      message = parseMessage(JSON.parse(data))
    } catch {
      // Not JSON: ignore.
    }
    if (message) handlers.onMessage(peerId, message)
  }
  room.onPeerJoin = handlers.onPeerJoin
  room.onPeerLeave = handlers.onPeerLeave
  return {
    send: (message, peerId) => void action.send(JSON.stringify(message), peerId ? { target: peerId } : undefined).catch(() => {}),
    leave: () => void room.leave().catch(() => {}),
  }
}
