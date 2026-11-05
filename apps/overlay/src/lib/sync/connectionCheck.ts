// The connection check: what stands between the player and their friends. It measures
// this side only (a failed link can't say which side blocked it), so the wording says
// what's known and where to look.

import { defaultRelayUrls } from 'trystero'
import { connectionOf } from '../together'
import type { Peer } from './sync'

/** Open: direct links work. Strict: a new port per destination, so two strict sides can't link. */
export type NetworkKind = 'open' | 'strict' | 'blocked'

/** How a connected peer is reached: inside the same network, or across the internet. */
export type Route = 'local' | 'internet'

export interface PeerProbe {
  route: Route | null
  ms: number | null
}

/** A peer that was found but could not be linked with. */
export type FailureKind = 'network' | 'mismatch' | 'other'

export interface PeerFailure {
  peerId: string
  kind: FailureKind
}

export interface ConnectionReport {
  /** How many of the servers used to find friends answered. */
  relays: { reached: number; tried: number }
  network: NetworkKind
  peers: Record<string, PeerProbe>
}

export type LineTone = 'ok' | 'warn' | 'bad'

export interface CheckLine {
  tone: LineTone
  title: string
  detail?: string
}

export interface CheckInput {
  report: ConnectionReport
  playTogether: boolean
  /** The multiplayer session, if the game reports one. */
  session: string | null
  /** Other players in the game. */
  players: { name: string }[]
  peers: Peer[]
  failures: PeerFailure[]
}

const STUN_SERVERS = ['stun:stun.l.google.com:19302', 'stun:stun.cloudflare.com:3478']
const STUN_TIMEOUT_MS = 5000
const RELAY_TIMEOUT_MS = 4000
/** Enough answers to know the player can reach them. */
const RELAYS_NEEDED = 3

/** What went wrong joining with a peer, from Trystero's message. */
export function classifyFailure(error: string): FailureKind {
  if (error.includes('after exchanging SDP')) return 'network'
  if (error.includes('password')) return 'mismatch'
  return 'other'
}

/** The parts of an ICE candidate line that say what the router did. */
function parseCandidate(line: string) {
  const parts = line.split(' ')
  const at = (key: string) => {
    const i = parts.indexOf(key)
    return i < 0 ? undefined : parts[i + 1]
  }
  return { protocol: parts[2]?.toLowerCase(), address: parts[4], port: parts[5], type: at('typ') }
}

/**
 * What the public addresses seen by several STUN servers say about the network. A router
 * that picks a new port for each destination is strict: more ports on one public address
 * than there are local sockets (WebView2 doesn't say which socket a port belongs to, and
 * two adapters behind one router look the same otherwise). None seen means UDP doesn't
 * get out.
 */
export function classifyNetwork(candidates: string[]): NetworkKind {
  const udp = candidates.map(parseCandidate).filter((c) => c.protocol === 'udp')
  const seen = udp.filter((c) => c.type === 'srflx')
  if (seen.length === 0) return 'blocked'
  const sockets = Math.max(udp.filter((c) => c.type === 'host').length, 1)
  const ports = new Map<string, Set<string>>()
  for (const c of seen) ports.set(c.address, (ports.get(c.address) ?? new Set()).add(c.port))
  return [...ports.values()].some((p) => p.size > sockets) ? 'strict' : 'open'
}

/** Asks STUN servers how the player looks from outside. */
export async function probeNetwork(): Promise<NetworkKind> {
  const pc = new RTCPeerConnection({ iceServers: STUN_SERVERS.map((urls) => ({ urls })) })
  const candidates: string[] = []
  try {
    pc.createDataChannel('check')
    const gathered = new Promise<void>((resolve) => {
      pc.onicecandidate = (e) => {
        if (e.candidate) candidates.push(e.candidate.candidate)
        else resolve()
      }
      setTimeout(resolve, STUN_TIMEOUT_MS)
    })
    await pc.setLocalDescription(await pc.createOffer())
    await gathered
  } finally {
    pc.close()
  }
  return classifyNetwork(candidates)
}

/** Tries the servers used to find friends, stopping once enough have answered. */
export function probeRelays(): Promise<ConnectionReport['relays']> {
  const tried = defaultRelayUrls.length
  return new Promise((resolve) => {
    const sockets: WebSocket[] = []
    let reached = 0
    let pending = tried
    const finish = () => {
      clearTimeout(timer)
      for (const s of sockets) s.close()
      resolve({ reached, tried })
    }
    const timer = setTimeout(finish, RELAY_TIMEOUT_MS)
    for (const url of defaultRelayUrls) {
      const socket = new WebSocket(url.startsWith('wss://') ? url : `wss://${url}`)
      sockets.push(socket)
      const settled = (ok: boolean) => {
        socket.onopen = socket.onerror = null
        if (ok) reached++
        if (--pending === 0 || reached >= RELAYS_NEEDED) finish()
      }
      socket.onopen = () => settled(true)
      socket.onerror = () => settled(false)
    }
  })
}

/** How the connection to a peer runs, from its selected candidate pair. */
export function routeFromStats(stats: Iterable<{ id: string; type: string; [key: string]: unknown }>): Route | null {
  const byId = new Map([...stats].map((s) => [s.id, s]))
  const all = [...byId.values()]
  const selected = all.find((s) => s.type === 'transport' && s.selectedCandidatePairId)?.selectedCandidatePairId
  const pair = byId.get(selected as string) ?? all.find((s) => s.type === 'candidate-pair' && s.state === 'succeeded' && s.nominated)
  if (!pair) return null
  const local = byId.get(pair.localCandidateId as string)
  const remote = byId.get(pair.remoteCandidateId as string)
  if (!local || !remote) return null
  return local.candidateType === 'host' && remote.candidateType === 'host' ? 'local' : 'internet'
}

const NETWORK_LINES: Record<NetworkKind, CheckLine> = {
  open: { tone: 'ok', title: 'Your network allows direct connections' },
  strict: {
    tone: 'warn',
    title: 'Your network looks strict',
    detail: "You can link with most friends, but not with another strict network. Mobile hotspots, VPNs and some internet providers do this. Try without the VPN, or on another connection.",
  },
  blocked: {
    tone: 'bad',
    title: 'Direct connections are blocked here',
    detail: 'Nothing got out to test the connection. Check your firewall, antivirus and VPN, and allow Game Maps through.',
  },
}

function peerLine(peer: Peer, probe: PeerProbe | undefined): CheckLine {
  const name = peer.name ?? 'A player'
  if (!peer.ready) return { tone: 'bad', title: `${name} needs to update Game Maps`, detail: 'You reached each other, but you need the same version to share anything.' }
  const route = probe?.route === 'local' ? 'same network' : probe?.route === 'internet' ? 'over the internet' : null
  const ms = probe?.ms != null ? `${Math.round(probe.ms)} ms` : null
  const how = [route, ms].filter(Boolean).join(', ')
  return { tone: 'ok', title: `Connected with ${name}`, detail: how || undefined }
}

const FAILURE_LINES: Record<FailureKind, (count: number) => CheckLine> = {
  network: (count) => ({
    tone: 'bad',
    title: count === 1 ? 'Found a player, but could not link up' : `Found ${count} players, but could not link up`,
    detail: 'A network on one side is blocking the link. Each of you should run this check: whoever shows a strict or blocked network is the side to fix.',
  }),
  mismatch: () => ({ tone: 'bad', title: 'A player is on a different session', detail: 'Rejoin the same game, then check again.' }),
  other: (count) => ({ tone: 'warn', title: count === 1 ? 'A connection attempt failed' : `${count} connection attempts failed`, detail: 'Check again in a minute. The logs have the details.' }),
}

/** The result, as lines to show: what's wrong first within each step. */
export function checkLines({ report, playTogether, session, players, peers, failures }: CheckInput): CheckLine[] {
  const { reached, tried } = report.relays
  const lines: CheckLine[] = [
    reached > 0
      ? { tone: 'ok', title: 'Reached the servers that find your friends' }
      : { tone: 'bad', title: `Could not reach the servers that find your friends (${tried} tried)`, detail: 'Check your internet connection, firewall and antivirus.' },
    NETWORK_LINES[report.network],
  ]
  if (!playTogether) return [...lines, { tone: 'bad', title: 'Play together is off', detail: 'Turn it on above to connect with friends.' }]
  if (!session) return [...lines, { tone: 'warn', title: 'Not in a multiplayer game yet', detail: 'Check again once you and your friends are in the same game.' }]

  lines.push(...peers.map((p) => peerLine(p, report.peers[p.id])))
  for (const kind of ['network', 'mismatch', 'other'] as const) {
    const count = failures.filter((f) => f.kind === kind).length
    if (count) lines.push(FAILURE_LINES[kind](count))
  }
  const missing = players.filter((p) => connectionOf(p, peers) === 'not-connected')
  if (missing.length) {
    lines.push({
      tone: 'warn',
      title: `Not connected with ${missing.map((p) => p.name).join(', ')}`,
      detail: 'They need Game Maps running and connected to the game, with Play together on.',
    })
  }
  if (peers.length === 0 && failures.length === 0 && missing.length === 0) lines.push({ tone: 'warn', title: 'Nobody else is here yet' })
  return lines
}
