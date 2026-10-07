import { forwardRef, type InputHTMLAttributes } from 'react'
import { cn } from '@/lib/utils/cn'

interface FieldInputProps extends InputHTMLAttributes<HTMLInputElement> {
  invalid?: boolean
}

/** Bare input for label-on-the-left settings rows (TextField is label-on-top). */
export const FieldInput = forwardRef<HTMLInputElement, FieldInputProps>(function FieldInput(
  { invalid, className, ...rest },
  ref,
) {
  return (
    <input
      ref={ref}
      aria-invalid={invalid || undefined}
      className={cn(
        'h-9 rounded-lg border bg-surface px-3 text-base text-ink transition-colors',
        'placeholder:text-ink-3 focus:outline-none',
        invalid ? 'border-danger/60 focus:border-danger' : 'border-line focus:border-accent/70',
        className,
      )}
      {...rest}
    />
  )
})
