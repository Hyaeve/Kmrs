import { cn } from '@/lib/utils/cn'

interface SegmentedControlProps<T extends string> {
  options: Array<{ value: T; label: string }>
  value: T
  onChange: (v: T) => void
  className?: string
  size?: 'sm' | 'md'
}

export function SegmentedControl<T extends string>({ options, value, onChange, className, size = 'md' }: SegmentedControlProps<T>) {
  return (
    <div className={cn('inline-flex items-center gap-0.5 rounded-lg border border-line bg-surface p-0.5', className)}>
      {options.map((opt) => (
        <button
          key={opt.value}
          type="button"
          onClick={() => onChange(opt.value)}
          className={cn(
            'cursor-pointer rounded-md font-medium whitespace-nowrap transition-all duration-150',
            size === 'sm' ? 'px-2.5 py-1 text-xs' : 'px-3 py-1.5 text-[13px]',
            value === opt.value ? 'bg-raised text-ink shadow-sm' : 'text-ink-3 hover:text-ink-2',
          )}
        >
          {opt.label}
        </button>
      ))}
    </div>
  )
}
