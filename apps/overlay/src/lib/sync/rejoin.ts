// When to give up on a joined room and join it again. Trystero reconnects its relays by
// itself, but a room can still end up connected to nobody: a failed link isn't retried,
// and friends can be missed while the network is down.

/** First wait before rejoining; doubles with each attempt. */
const FIRST_MS = 30_000
const MAX_MS = 5 * 60_000

export function rejoinDelay(attempts: number): number {
  return Math.min(FIRST_MS * 2 ** Math.min(attempts, 10), MAX_MS)
}

export interface RoomState {
  /** Connected peers. */
  peers: number
  /** Other players in the game: there's someone to find. */
  players: number
  /** Peers found but not linked with. */
  failures: number
  /** How long this attempt has been going. */
  waitedMs: number
  /** Rejoins since a peer was last connected. */
  attempts: number
}

/** Only with nobody connected: rejoining would drop the friends who are. */
export function shouldRejoin({ peers, players, failures, waitedMs, attempts }: RoomState): boolean {
  return peers === 0 && (players > 0 || failures > 0) && waitedMs >= rejoinDelay(attempts)
}
