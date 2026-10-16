import type { ReactNode } from 'react'
import { Link } from 'react-router-dom'
import { cn } from '@/lib/utils/cn'

interface ChipProps {
  children: ReactNode
  to?: string
  onClick?: () => void
  icon?: ReactNode
  className?: string
}

/** Metadata chip: pill, soft accent-free surface, optional link. */
export function Chip({ children, to, onClick, icon, className }: ChipProps) {
  const cls = cn(
    'inline-flex items-center gap-1.5 rounded-full border border-line bg-raised px-3 py-1 text-xs text-ink-2 transition-colors',
    (to || onClick) && 'cursor-pointer hover:border-accent/50 hover:text-accent-strong',
    className,
  )
  const inner = (
    <>
      {icon && <span className="inline-flex text-ink-3">{icon}</span>}
      {children}
    </>
  )
  if (to)
    return (
      <Link to={to} className={cls}>
        {inner}
      </Link>
    )
  if (onClick)
    return (
      <button type="button" onClick={onClick} className={cls}>
        {inner}
      </button>
    )
  return <span className={cls}>{inner}</span>
}
