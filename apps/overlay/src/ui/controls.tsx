// Small shared UI primitives.

import type { ButtonHTMLAttributes, ReactNode } from 'react'

export function IconButton({
  label,
  active,
  className = '',
  children,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { label: string; active?: boolean; children: ReactNode }) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      aria-pressed={active}
      className={`grid size-9 place-items-center rounded-lg transition-colors ${
        active ? 'bg-accent text-accent-ink' : 'text-ink-dim hover:bg-panel-raised hover:text-ink'
      } disabled:opacity-35 disabled:hover:bg-transparent ${className}`}
      {...rest}
    >
      {children}
    </button>
  )
}

export function Button({
  tone = 'default',
  className = '',
  children,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { tone?: 'default' | 'primary' | 'danger'; children: ReactNode }) {
  const tones = {
    default: 'bg-panel-raised text-ink hover:bg-white/12',
    primary: 'bg-accent text-accent-ink hover:brightness-110',
    danger: 'bg-danger/15 text-danger hover:bg-danger/25',
  }
  return (
    <button
      type="button"
      className={`inline-flex items-center justify-center gap-1.5 rounded-lg px-3 py-1.5 text-[13px] font-semibold transition disabled:cursor-not-allowed disabled:opacity-40 ${tones[tone]} ${className}`}
      {...rest}
    >
      {children}
    </button>
  )
}

export function Slider({
  label,
  value,
  min,
  max,
  step,
  format,
  onChange,
}: {
  label: string
  value: number
  min: number
  max: number
  step: number
  format?: (v: number) => string
  onChange: (v: number) => void
}) {
  return (
    <label className="block">
      <div className="mb-1 flex justify-between text-[12px] text-ink-dim">
        <span>{label}</span>
        <span className="tabular-nums text-ink">{format ? format(value) : value}</span>
      </div>
      <input
        type="range"
        className="w-full accent-[var(--color-accent)]"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
      />
    </label>
  )
}

export function Toggle({ label, checked, onChange, hint }: { label: string; checked: boolean; onChange: (v: boolean) => void; hint?: string }) {
  return (
    <label className="flex cursor-pointer items-start justify-between gap-3 py-1">
      <span>
        <span className="block text-[13px]">{label}</span>
        {hint && <span className="block text-[11px] text-ink-faint">{hint}</span>}
      </span>
      <input type="checkbox" className="mt-0.5 size-4 accent-[var(--color-accent)]" checked={checked} onChange={(e) => onChange(e.target.checked)} />
    </label>
  )
}
