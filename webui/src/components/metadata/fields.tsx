import { useId, type InputHTMLAttributes, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { LockSimple, LockSimpleOpen, Plus, X } from '@phosphor-icons/react'
import { cn } from '@/lib/utils/cn'
import { Button } from '@/components/ui/Button'
import { IconButton } from '@/components/ui/IconButton'
import { Switch } from '@/components/ui/Switch'

/** Metadata field lock toggle: locked fields are protected from metadata refresh. */
export function LockToggle({
  locked,
  onChange,
  label,
  className,
}: {
  locked: boolean
  onChange: (v: boolean) => void
  label: string
  className?: string
}) {
  const { t } = useTranslation('metadata')
  const action = locked ? t('lockToggle.unlock', { label }) : t('lockToggle.lock', { label })
  return (
    <button
      type="button"
      onClick={() => onChange(!locked)}
      aria-pressed={locked}
      aria-label={action}
      title={action}
      className={cn(
        'inline-flex shrink-0 cursor-pointer items-center justify-center rounded-md p-1 transition-colors hover:bg-overlay',
        locked ? 'text-accent' : 'text-ink-3',
        className,
      )}
    >
      {locked ? <LockSimple className="size-4" weight="bold" /> : <LockSimpleOpen className="size-4" />}
    </button>
  )
}

export function FormErrorBanner({ messages }: { messages: string[] }) {
  if (messages.length === 0) return null
  return (
    <div className="flex flex-col gap-1 rounded-lg border border-danger/40 bg-danger/10 px-3 py-2.5 text-[13px] text-danger">
      {messages.map((m, i) => (
        <p key={i}>{m}</p>
      ))}
    </div>
  )
}

interface TextInputProps extends InputHTMLAttributes<HTMLInputElement> {
  error?: boolean
}

/** Bare styled input for rows and batch fields; labelled forms should use ui/TextField. */
export function TextInput({ error, className, ...rest }: TextInputProps) {
  return (
    <input
      aria-invalid={!!error}
      className={cn(
        'h-10 w-full rounded-lg border bg-surface px-3 text-base text-ink transition-colors',
        'placeholder:text-ink-3 focus:outline-none',
        error ? 'border-danger/60 focus:border-danger' : 'border-line focus:border-accent/70',
        className,
      )}
      {...rest}
    />
  )
}

interface TextAreaFieldProps {
  label?: string
  value: string
  onChange: (v: string) => void
  error?: string
  helper?: string
  placeholder?: string
  rows?: number
  disabled?: boolean
  ariaLabel?: string
  /** rendered at the right of the label row (e.g. a LockToggle) */
  trailing?: ReactNode
}

export function TextAreaField({ label, value, onChange, error, helper, placeholder, rows = 4, disabled, ariaLabel, trailing }: TextAreaFieldProps) {
  const id = useId()
  return (
    <div className="flex flex-col gap-2">
      {label && (
        <div className="flex items-center justify-between gap-2">
          <label htmlFor={id} className="text-[13px] font-medium text-ink-2">
            {label}
          </label>
          {trailing}
        </div>
      )}
      <textarea
        id={id}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        rows={rows}
        disabled={disabled}
        aria-label={ariaLabel}
        aria-invalid={!!error}
        className={cn(
          'w-full resize-y rounded-lg border bg-surface px-3 py-2 text-base text-ink transition-colors',
          'placeholder:text-ink-3 focus:outline-none',
          error ? 'border-danger/60 focus:border-danger' : 'border-line focus:border-accent/70',
        )}
      />
      {error ? <p className="text-[13px] text-danger">{error}</p> : helper ? <p className="text-[13px] text-ink-3">{helper}</p> : null}
    </div>
  )
}

/** A labelled group of controls that stays out of the PATCH body until explicitly switched on. */
export function BatchField({
  label,
  enabled,
  onEnabledChange,
  hint,
  locked,
  onLockedChange,
  children,
}: {
  label: string
  enabled: boolean
  onEnabledChange: (v: boolean) => void
  hint?: string
  /** when both are given, a lock toggle is shown next to the switch while the field is on */
  locked?: boolean
  onLockedChange?: (v: boolean) => void
  children: ReactNode
}) {
  const { t } = useTranslation('metadata')
  return (
    <div className={cn('rounded-lg border border-line p-3.5 transition-colors', enabled && 'border-accent/40')}>
      <div className="flex items-center justify-between gap-4">
        <span className="text-[13px] font-medium text-ink-2">{label}</span>
        <div className="flex items-center gap-1">
          {enabled && locked !== undefined && onLockedChange && <LockToggle locked={locked} onChange={onLockedChange} label={label} />}
          <Switch checked={enabled} onCheckedChange={onEnabledChange} label={t('batch.change', { label })} />
        </div>
      </div>
      {enabled && (
        <div className="mt-3">
          {children}
          {hint && <p className="mt-2 text-xs text-ink-3">{hint}</p>}
        </div>
      )}
    </div>
  )
}

export interface Pair {
  a: string
  b: string
}

export type PairErrors = Record<number, { a?: string; b?: string }>

interface PairListEditorProps {
  label?: string
  pairs: Pair[]
  onChange: (pairs: Pair[]) => void
  aLabel: string
  bLabel: string
  aPlaceholder?: string
  bPlaceholder?: string
  addLabel: string
  errors?: PairErrors
  /** rendered at the right of the label row (e.g. a LockToggle) */
  trailing?: ReactNode
}

/** Two-column list editor (label/title, label/url, name/role) with per-row errors. */
export function PairListEditor({ label, pairs, onChange, aLabel, bLabel, aPlaceholder, bPlaceholder, addLabel, errors, trailing }: PairListEditorProps) {
  const { t } = useTranslation('metadata')
  const update = (i: number, key: 'a' | 'b', v: string) => onChange(pairs.map((p, j) => (j === i ? { ...p, [key]: v } : p)))
  return (
    <div className="flex flex-col gap-2">
      {label && (
        <div className="flex items-center justify-between gap-2">
          <span className="text-[13px] font-medium text-ink-2">{label}</span>
          {trailing}
        </div>
      )}
      {pairs.length > 0 && (
        <div className="flex flex-col gap-3">
          {pairs.map((p, i) => (
            <div key={i} className="flex items-start gap-2">
              <div className="grid flex-1 gap-2 sm:grid-cols-2">
                <div>
                  <label className="mb-1 block text-xs text-ink-3" htmlFor={`pair-${aLabel}-${i}-a`}>
                    {aLabel}
                  </label>
                  <TextInput
                    id={`pair-${aLabel}-${i}-a`}
                    value={p.a}
                    onChange={(e) => update(i, 'a', e.target.value)}
                    placeholder={aPlaceholder}
                    error={!!errors?.[i]?.a}
                  />
                  {errors?.[i]?.a && <p className="mt-1 text-[13px] text-danger">{errors[i].a}</p>}
                </div>
                <div>
                  <label className="mb-1 block text-xs text-ink-3" htmlFor={`pair-${bLabel}-${i}-b`}>
                    {bLabel}
                  </label>
                  <TextInput
                    id={`pair-${bLabel}-${i}-b`}
                    value={p.b}
                    onChange={(e) => update(i, 'b', e.target.value)}
                    placeholder={bPlaceholder}
                    error={!!errors?.[i]?.b}
                  />
                  {errors?.[i]?.b && <p className="mt-1 text-[13px] text-danger">{errors[i].b}</p>}
                </div>
              </div>
              <IconButton label={t('pairs.removeRow')} className="mt-6" onClick={() => onChange(pairs.filter((_, j) => j !== i))}>
                <X className="size-4" />
              </IconButton>
            </div>
          ))}
        </div>
      )}
      <div>
        <Button type="button" size="sm" onClick={() => onChange([...pairs, { a: '', b: '' }])}>
          <Plus className="size-4" />
          {addLabel}
        </Button>
      </div>
    </div>
  )
}
