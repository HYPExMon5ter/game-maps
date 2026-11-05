// Who in the game is connected through Game Maps, for the players list.

import type { RemotePlayer } from './protocol'
import type { Peer } from './sync/sync'

export type Connection =
  /** Connected and on the same message version. */
  | 'connected'
  /** Connected, but on a version we can't talk to: they need to update. */
  | 'outdated'
  /** In the game, not connected to us. */
  | 'not-connected'

/** How the app is connected to `player`, matched on the name both report. */
export function connectionOf(player: Pick<RemotePlayer, 'name'>, peers: Peer[]): Connection {
  const peer = peers.find((p) => p.name === player.name)
  if (!peer) return 'not-connected'
  return peer.ready ? 'connected' : 'outdated'
}

export const CONNECTION_LABELS: Record<Connection, string> = {
  connected: 'Connected',
  outdated: 'Needs to update Game Maps',
  'not-connected': 'Not connected',
}
