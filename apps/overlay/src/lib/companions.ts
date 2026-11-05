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

/** Whether a stat is high when things are bad (shown in a warning color when high). */
export function isBadStat(name: string): boolean {
  return name === 'Fear' || name === 'Anger'
}
