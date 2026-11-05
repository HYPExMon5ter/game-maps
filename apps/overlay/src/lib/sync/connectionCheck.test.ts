import { describe, expect, it } from 'vitest'
import { checkLines, classifyFailure, classifyNetwork, routeFromStats, type CheckInput, type ConnectionReport } from './connectionCheck'
import type { Peer } from './sync'

// As WebView2 reports them: hidden local addresses, no network-id.
const srflx = (address: string, port: number) => `candidate:1 1 udp 1677729535 ${address} ${port} typ srflx raddr 0.0.0.0 rport 0 generation 0 ufrag fbwi network-cost 999`
const host = 'candidate:2 1 udp 2113937151 abc.local 50000 typ host generation 0 ufrag fbwi network-cost 999'

describe('classifyNetwork', () => {
  it('is open when every server sees the same port', () => {
    expect(classifyNetwork([host, srflx('1.2.3.4', 4000)])).toBe('open')
  })

  it('is strict when servers see different ports from one adapter', () => {
    expect(classifyNetwork([host, srflx('1.2.3.4', 4000), srflx('1.2.3.4', 4001)])).toBe('strict')
  })

  it('does not mistake two adapters for a strict router', () => {
    expect(classifyNetwork([host, host, srflx('1.2.3.4', 4000), srflx('1.2.3.4', 4001)])).toBe('open')
  })

  it('sees a strict router behind two adapters', () => {
    expect(classifyNetwork([host, host, srflx('1.2.3.4', 4000), srflx('1.2.3.4', 4001), srflx('1.2.3.4', 4002)])).toBe('strict')
  })

  it('is blocked when nothing came back from outside', () => {
    expect(classifyNetwork([host])).toBe('blocked')
    expect(classifyNetwork([])).toBe('blocked')
  })
})

describe('classifyFailure', () => {
  it('reads Trystero errors', () => {
    expect(classifyFailure('could not connect to peer x after exchanging SDP; configure TURN servers')).toBe('network')
    expect(classifyFailure('incorrect room password when decrypting offer')).toBe('mismatch')
    expect(classifyFailure('handshake timed out')).toBe('other')
  })
})

describe('routeFromStats', () => {
  const stats = (local: string, remote: string) => [
    { id: 't', type: 'transport', selectedCandidatePairId: 'p' },
    { id: 'p', type: 'candidate-pair', localCandidateId: 'l', remoteCandidateId: 'r' },
    { id: 'l', type: 'local-candidate', candidateType: local },
    { id: 'r', type: 'remote-candidate', candidateType: remote },
  ]

  it('tells the same network from the internet', () => {
    expect(routeFromStats(stats('host', 'host'))).toBe('local')
    expect(routeFromStats(stats('srflx', 'prflx'))).toBe('internet')
  })

  it('knows nothing without a selected pair', () => {
    expect(routeFromStats([{ id: 't', type: 'transport' }])).toBeNull()
  })
})

describe('checkLines', () => {
  const report: ConnectionReport = { relays: { reached: 3, tried: 10 }, network: 'open', peers: {} }
  const friend: Peer = { id: 'a', name: 'Sam', ready: true }
  const input = (patch: Partial<CheckInput>): CheckInput => ({ report, playTogether: true, session: 'lobby', players: [], peers: [], failures: [], ...patch })
  const titles = (patch: Partial<CheckInput>) => checkLines(input(patch)).map((l) => l.title)

  it('stops at what has to change first', () => {
    expect(titles({ playTogether: false }).at(-1)).toBe('Play together is off')
    expect(titles({ session: null }).at(-1)).toBe('Not in a multiplayer game yet')
  })

  it('says how a friend is connected', () => {
    const lines = checkLines(input({ peers: [friend], players: [{ name: 'Sam' }], report: { ...report, peers: { a: { route: 'internet', ms: 41.6 } } } }))
    expect(lines.at(-1)).toEqual({ tone: 'ok', title: 'Connected with Sam', detail: 'over the internet, 42 ms' })
  })

  it('names who needs to update', () => {
    expect(titles({ peers: [{ ...friend, ready: false }], players: [{ name: 'Sam' }] }).at(-1)).toBe('Sam needs to update Game Maps')
  })

  it('names players in the game who are not connected', () => {
    expect(titles({ players: [{ name: 'Sam' }, { name: 'Alex' }] }).at(-1)).toBe('Not connected with Sam, Alex')
  })

  it('reports friends found but not linked with', () => {
    const lines = titles({ failures: [{ peerId: 'x', kind: 'network' }, { peerId: 'y', kind: 'network' }] })
    expect(lines).toContain('Found 2 players, but could not link up')
    expect(lines).not.toContain('Nobody else is here yet')
  })

  it('flags unreachable servers', () => {
    expect(checkLines(input({ report: { ...report, relays: { reached: 0, tried: 10 } } }))[0].tone).toBe('bad')
  })
})
