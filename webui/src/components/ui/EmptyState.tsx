import type { ReactNode } from 'react'
import { cn } from '@/lib/utils/cn'

interface EmptyStateProps {
  icon?: ReactNode
  title: string
  body?: string
  action?: ReactNode
  className?: string
}

export function EmptyState({ icon, title, body, action, className }: EmptyStateProps) {
  return (
    <div className={cn('flex flex-col items-center justify-center gap-2 px-6 py-16 text-center', className)}>
      {icon && <div className="mb-1 text-ink-3 [&_svg]:size-10">{icon}</div>}
      <h3 className="text-[15px] font-semibold text-ink">{title}</h3>
      {body && <p className="max-w-sm text-sm leading-relaxed text-ink-3">{body}</p>}
      {action && <div className="mt-3">{action}</div>}
    </div>
  )
}
