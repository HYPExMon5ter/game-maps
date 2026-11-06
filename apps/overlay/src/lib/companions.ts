// Companion details and the order menu built from what the agent reports.

import type { CompanionOrderOption, CompanionStatus, WorldSnapshot } from './protocol'

/** Prefix of the groups that fetch something ("Get logs"); their orders say where to. */
const FETCH_PREFIX = 'Get '

export interface OrderGroup {
  title: string
  orders: CompanionOrderOption[]
}

/** Something a companion can fetch, and the ways of delivering it. */
export interface FetchItem {
  item: string
  drops: CompanionOrderOption[]
}

export interface OrderMenu {
  groups: OrderGroup[]
  fetch: FetchItem[]
}

/** The orders as a menu: fetching kept apart, as it takes two choices. */
export function orderMenu(orders: CompanionOrderOption[]): OrderMenu {
  const groups = new Map<string, CompanionOrderOption[]>()
  const fetch = new Map<string, CompanionOrderOption[]>()
  for (const order of orders) {
    const into = order.group.startsWith(FETCH_PREFIX) ? fetch : groups
    const key = order.group.startsWith(FETCH_PREFIX) ? order.group.slice(FETCH_PREFIX.length) : order.group
    into.set(key, [...(into.get(key) ?? []), order])
  }
  return {
    groups: [...groups].map(([title, list]) => ({ title, orders: list })),
    fetch: [...fetch].map(([item, drops]) => ({ item, drops })),
  }
}

/** The details the game reports for the companion called `name`. */
export function companionStatus(snapshot: WorldSnapshot | null, name: string): CompanionStatus | undefined {
  return snapshot?.companions?.find((c) => c.name === name)
}

/** One line on how a companion is doing. */
export function companionState(c: CompanionStatus): string {
  if (!c.alive) return 'Dead'
  if (c.injured) return 'Injured, needs helping up'
  return c.activity ?? 'Idle'
}

/** Stats that hurt a companion when they run out. */
const NEEDS = ['Health', 'Fullness', 'Hydration']
/** Share of a stat that warns, and that it has to recover to for the next warning. */
const LOW_SHARE = 0.25
const RECOVERED_SHARE = 0.4

/**
 * What's newly wrong with companions, as messages. `warned` remembers what's been said, so
 * a stat hovering around the limit doesn't warn over and over.
 */
export function companionAlerts(companions: CompanionStatus[], warned: Set<string>): string[] {
  const alerts: string[] = []
  const check = (key: string, bad: boolean, good: boolean, message: string) => {
    if (bad && !warned.has(key)) {
      warned.add(key)
      alerts.push(message)
    } else if (good) {
      warned.delete(key)
    }
  }
  for (const c of companions) {
    check(`${c.id}:dead`, !c.alive, c.alive, `${c.name} died`)
    if (!c.alive) continue
    check(`${c.id}:injured`, c.injured, !c.injured, `${c.name} needs helping up`)
    for (const stat of c.stats) {
      if (!NEEDS.includes(stat.name) || stat.max <= 0) continue
      const share = stat.value / stat.max
      check(`${c.id}:${stat.name}`, share < LOW_SHARE, share >= RECOVERED_SHARE, `${c.name}'s ${stat.name.toLowerCase()} is low`)
    }
  }
  return alerts
}

/** Whether a stat is high when things are bad (shown in a warning color when high). */
export function isBadStat(name: string): boolean {
  return name === 'Fear' || name === 'Anger'
}
