import { useState, type ReactNode } from 'react'
import { CaretRight } from '@phosphor-icons/react'
import { cn } from '@/lib/utils/cn'

interface ChecklistProps<T extends string> {
  options: ReadonlyArray<{ value: T; label: string }>
  values: T[]
  onChange: (values: T[]) => void
  className?: string
}

/** Fixed-option multi-select rendered as a checkbox grid. */
export function Checklist<T extends string>({ options, values, onChange, className }: ChecklistProps<T>) {
  const toggle = (v: T, on: boolean) => onChange(on ? [...values, v] : values.filter((x) => x !== v))
  return (
    <div className={cn('grid gap-1.5 sm:grid-cols-2', className)}>
      {options.map((opt) => (
        <label key={opt.value} className="flex cursor-pointer items-center gap-2 text-sm text-ink-2">
          <input
            type="checkbox"
            checked={values.includes(opt.value)}
            onChange={(e) => toggle(opt.value, e.target.checked)}
            className="size-4 shrink-0 cursor-pointer accent-accent"
          />
          <span className="truncate">{opt.label}</span>
        </label>
      ))}
    </div>
  )
}

interface StringListInputProps {
  value: string[]
  onChange: (value: string[]) => void
  'aria-label': string
  placeholder?: string
  rows?: number
}

/** One-value-per-line editor for free-form string lists; blank lines are dropped at save time. */
export function StringListInput({ value, onChange, rows = 3, placeholder, 'aria-label': ariaLabel }: StringListInputProps) {
  return (
    <textarea
      aria-label={ariaLabel}
      rows={rows}
      value={value.join('\n')}
      onChange={(e) => onChange(e.target.value.split('\n'))}
      placeholder={placeholder}
      className="w-full rounded-lg border border-line bg-surface px-3 py-2 font-mono text-[13px] text-ink placeholder:text-ink-3 focus:border-accent/70 focus:outline-none"
    />
  )
}

/** Divider-topped group heading inside a settings card, e.g. "Post-processing". */
export function Subheading({ children }: { children: string }) {
  return <h3 className="mt-4 border-t border-line pt-4 text-[13px] font-medium text-ink">{children}</h3>
}

interface SelectInputProps<T extends string> {
  options: ReadonlyArray<{ value: T; label: string }>
  value: T
  onChange: (value: T) => void
  'aria-label': string
  className?: string
}

export function SelectInput<T extends string>({
  options,
  value,
  onChange,
  className,
  'aria-label': ariaLabel,
}: SelectInputProps<T>) {
  return (
    <select
      aria-label={ariaLabel}
      value={value}
      onChange={(e) => onChange(e.target.value as T)}
      className={cn(
        'h-9 cursor-pointer rounded-lg border border-line bg-surface px-3 text-base text-ink focus:border-accent/70 focus:outline-none',
        className,
      )}
    >
      {options.map((opt) => (
        <option key={opt.value} value={opt.value}>
          {opt.label}
        </option>
      ))}
    </select>
  )
}

interface CollapsibleGroupProps {
  label: string
  summary?: string
  children: ReactNode
}

/** Collapsible counterpart to Subheading, for bulky field groups that are rarely edited. */
export function CollapsibleGroup({ label, summary, children }: CollapsibleGroupProps) {
  const [open, setOpen] = useState(false)
  return (
    <div className="mt-4 border-t border-line">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        className="flex w-full cursor-pointer items-center gap-2 py-3 text-left"
      >
        <CaretRight
          className={cn('size-3.5 shrink-0 text-ink-3 transition-transform duration-150', open && 'rotate-90')}
        />
        <span className="flex-1 text-sm text-ink-2">{label}</span>
        {summary && <span className="text-xs text-ink-3">{summary}</span>}
      </button>
      {open && <div className="pb-3">{children}</div>}
    </div>
  )
}
