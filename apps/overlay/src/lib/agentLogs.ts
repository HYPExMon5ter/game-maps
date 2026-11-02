// Ring buffer of agent diagnostics, collected from app start so the settings panel
// can show what happened before it was opened.

import { events } from './backend'
import type { AgentLog } from './protocol'

const MAX = 200
const buffer: AgentLog[] = []

export function startAgentLogBuffer(): Promise<() => void> {
  return events.agentLog((l) => {
    buffer.push(l)
    if (buffer.length > MAX) buffer.shift()
  })
}

export function agentLogs(): AgentLog[] {
  return [...buffer]
}
