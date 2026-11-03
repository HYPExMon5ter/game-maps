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
  label: ReactNode
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

/** On/off switch. Put it inside a `<label>` (clicking the text toggles it) or give it `label`. */
export function Switch({
  checked,
  onChange,
  label,
  disabled,
  className = '',
}: {
  checked: boolean
  onChange: (v: boolean) => void
  label?: string
  disabled?: boolean
  className?: string
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={`relative inline-flex h-5 w-9 shrink-0 items-center rounded-full transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent disabled:cursor-not-allowed disabled:opacity-40 ${
        checked ? 'bg-accent' : 'bg-white/18'
      } ${className}`}
    >
      <span className={`size-4 rounded-full bg-white shadow transition-transform ${checked ? 'translate-x-[18px]' : 'translate-x-0.5'}`} />
    </button>
  )
}

export function Toggle({
  label,
  checked,
  onChange,
  hint,
  disabled,
}: {
  label: ReactNode
  checked: boolean
  onChange: (v: boolean) => void
  hint?: ReactNode
  disabled?: boolean
}) {
  return (
    <label className={`flex items-start justify-between gap-3 py-1 ${disabled ? 'cursor-not-allowed opacity-60' : 'cursor-pointer'}`}>
      <span>
        <span className="block text-[13px]">{label}</span>
        {hint && <span className="block text-[11px] text-ink-faint">{hint}</span>}
      </span>
      <Switch checked={checked} onChange={onChange} disabled={disabled} />
    </label>
  )
}

/** A choice between a few options, as a row of buttons. */
export function Segmented<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string
  value: T
  options: { id: T; label: string; title?: string }[]
  onChange: (value: T) => void
}) {
  return (
    <div className="inline-flex shrink-0 rounded-lg bg-black/30 p-0.5" role="radiogroup" aria-label={label}>
      {options.map((o) => (
        <button
          key={o.id}
          type="button"
          role="radio"
          title={o.title}
          aria-checked={value === o.id}
          className={`rounded-md px-2.5 py-1 text-[12px] font-semibold transition-colors ${
            value === o.id ? 'bg-accent text-accent-ink' : 'text-ink-dim hover:text-ink'
          }`}
          onClick={() => onChange(o.id)}
        >
          {o.label}
        </button>
      ))}
    </div>
  )
}
