// A companion's condition and, when they take orders, the menu to give them: the same
// orders as in the game, from the map.

import { ChevronDown } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { backend } from '../lib/backend'
import { companionState, companionStatus, isBadStat, orderMenu } from '../lib/companions'
import type { CompanionOrderOption, CompanionStat } from '../lib/protocol'
import { useStore } from '../state/store'
import { Button } from './controls'

function StatBar({ stat }: { stat: CompanionStat }) {
  const share = stat.max > 0 ? Math.min(stat.value / stat.max, 1) : 0
  const warn = isBadStat(stat.name) ? share > 0.5 : share < 0.25
  return (
    <div>
      <div className="mb-0.5 flex justify-between text-[11px] text-ink-faint">
        <span>{stat.name}</span>
        <span className="tabular-nums text-ink-dim">{Math.round(stat.value)}</span>
      </div>
      <div className="h-1.5 overflow-hidden rounded-full bg-black/40" role="presentation">
        <div className={`h-full rounded-full ${warn ? 'bg-danger' : 'bg-accent'}`} style={{ width: `${Math.round(share * 100)}%` }} />
      </div>
    </div>
  )
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  const [open, setOpen] = useState(false)
  return (
    <div>
      <button className="flex w-full items-center justify-between py-1 text-[12px] text-ink-dim hover:text-ink" aria-expanded={open} onClick={() => setOpen(!open)}>
        {title}
        <ChevronDown className={`size-3.5 transition-transform ${open ? 'rotate-180' : ''}`} />
      </button>
      {open && <div className="flex flex-wrap gap-1.5 pb-1.5">{children}</div>}
    </div>
  )
}

export function CompanionPanel({ name }: { name: string }) {
  const status = useStore((s) => companionStatus(s.snapshot, name))
  const notify = useStore((s) => s.notify)
  const [orders, setOrders] = useState<CompanionOrderOption[]>([])
  const [fetching, setFetching] = useState<string | null>(null)
  const id = status?.id
  const injured = status?.injured
  useEffect(() => {
    if (!id) return
    let cancelled = false
    backend.companionOrders(id).then(
      (list) => !cancelled && setOrders(list),
      // Companions that take no orders only show their condition.
      () => !cancelled && setOrders([]),
    )
    return () => {
      cancelled = true
    }
  }, [id, injured])
  const menu = useMemo(() => orderMenu(orders), [orders])
  if (!status) return null

  const give = (order: CompanionOrderOption, label = order.label) => {
    backend.companionOrder(status.id, order.id).then(
      () => notify(`${status.name}: ${label}`),
      (e) => notify(`${status.name} can't do that: ${String(e)}`, 'error'),
    )
  }
  const basics = menu.groups.find((g) => g.title === 'Basics')
  const rest = menu.groups.filter((g) => g !== basics)
  const item = menu.fetch.find((f) => f.item === fetching)

  return (
    <div className="mt-3 border-t border-line pt-3">
      <p className="mb-2 text-[12px] text-ink-dim">{companionState(status)}</p>
      {status.stats.length > 0 && (
        <div className="mb-3 grid grid-cols-2 gap-x-4 gap-y-1.5">
          {status.stats.map((stat) => (
            <StatBar key={stat.name} stat={stat} />
          ))}
        </div>
      )}
      {basics && (
        <div className="mb-1 flex flex-wrap gap-1.5">
          {basics.orders.map((o) => (
            <Button key={o.id} onClick={() => give(o)}>
              {o.label}
            </Button>
          ))}
        </div>
      )}
      {menu.fetch.length > 0 && (
        <Section title="Get">
          {menu.fetch.map((f) => (
            <Button key={f.item} tone={f.item === fetching ? 'primary' : 'default'} onClick={() => setFetching(f.item === fetching ? null : f.item)}>
              {f.item[0].toUpperCase() + f.item.slice(1)}
            </Button>
          ))}
          {item && (
            <div className="flex w-full flex-wrap gap-1.5 pt-1">
              {item.drops.map((d) => (
                <Button key={d.id} onClick={() => give(d, `get ${item.item}, ${d.label.toLowerCase()}`)}>
                  {d.label}
                </Button>
              ))}
            </div>
          )}
        </Section>
      )}
      {rest.map((g) => (
        <Section key={g.title} title={g.title}>
          {g.orders.map((o) => (
            <Button key={o.id} onClick={() => give(o, `${g.title.toLowerCase()}: ${o.label.toLowerCase()}`)}>
              {o.label}
            </Button>
          ))}
        </Section>
      ))}
    </div>
  )
}
