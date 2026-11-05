// Problem reports: what a window adds to the one the backend writes (see `report.rs`).

import { useStore } from '../state/store'
import { backend } from './backend'
import { useSyncStore } from './sync/sync'

/** What this window knows, without anything identifying a session or a player. */
function windowState(): string {
  const { game, mode, world, snapshot, status } = useStore.getState()
  const { session, peers, failures, report } = useSyncStore.getState()
  return JSON.stringify(
    {
      window: window.location.pathname + window.location.search,
      screen: [window.screen.width, window.screen.height],
      scale: window.devicePixelRatio,
      map: game?.id ?? null,
      mode,
      link: status?.state ?? null,
      worldKind: world.kind ?? null,
      location: !!snapshot?.local,
      otherPlayers: snapshot?.others.length ?? 0,
      playTogether: { joined: session !== null, peers: Object.values(peers).map((p) => ({ ready: p.ready })), failures: failures.map((f) => f.kind), lastCheck: report && { ...report, peers: Object.values(report.peers) } },
    },
    null,
    2,
  )
}

/** Asks where to save a report; true once saved. */
export function saveProblemReport(): Promise<boolean> {
  return backend.saveReport(windowState())
}
