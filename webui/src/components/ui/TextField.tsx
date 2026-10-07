import { forwardRef, useId, type InputHTMLAttributes, type ReactNode } from 'react'
import { cn } from '@/lib/utils/cn'

interface TextFieldProps extends InputHTMLAttributes<HTMLInputElement> {
  label: string
  helper?: string
  error?: string
  trailing?: ReactNode
}

export const TextField = forwardRef<HTMLInputElement, TextFieldProps>(function TextField(
  { label, helper, error, trailing, className, id, ...rest },
  ref,
) {
  const autoId = useId()
  const inputId = id ?? autoId
  return (
    <div className={cn('flex flex-col gap-2', className)}>
      <label htmlFor={inputId} className="text-[13px] font-medium text-ink-2">
        {label}
      </label>
      <div className="relative">
        <input
          ref={ref}
          id={inputId}
          className={cn(
            'h-10 w-full rounded-lg border bg-surface px-3 text-base text-ink transition-colors',
            'placeholder:text-ink-3 focus:outline-none disabled:opacity-50',
            error ? 'border-danger/60 focus:border-danger' : 'border-line focus:border-accent/70',
            trailing && 'pr-10',
          )}
          aria-invalid={!!error}
          {...rest}
        />
        {trailing && <div className="absolute inset-y-0 right-2 flex items-center">{trailing}</div>}
      </div>
      {error ? (
        <p className="text-[13px] text-danger">{error}</p>
      ) : helper ? (
        <p className="text-[13px] text-ink-3">{helper}</p>
      ) : null}
    </div>
  )
})
