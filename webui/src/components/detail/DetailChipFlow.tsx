import { useState, type ReactNode } from 'react'
import { cn } from '@/lib/utils/cn'
import { Chip } from '@/components/ui/Chip'
import { FadeKey } from '@/components/ui/FadeKey'

export interface DetailChipItem {
  key: string
  label: ReactNode
  icon?: ReactNode
  /** internal browse-filter link */
  to?: string
  className?: string
}

/** Flow of metadata chips with per-category icons; long lists collapse behind a "+N" chip that expands in place. */
export function DetailChipFlow({
  items,
  collapsedLimit = 12,
  className,
}: {
  items: DetailChipItem[]
  collapsedLimit?: number
  className?: string
}) {
  const [expanded, setExpanded] = useState(false)
  if (items.length === 0) return null
  const collapsed = !expanded && items.length > collapsedLimit
  const shown = collapsed ? items.slice(0, collapsedLimit) : items
  return (
    <FadeKey id={items.map((i) => i.key).join('|')} className={cn('flex flex-wrap gap-1.5', className)}>
      {shown.map((item) => (
        <Chip key={item.key} to={item.to} icon={item.icon} className={item.className}>
          {item.label}
        </Chip>
      ))}
      {collapsed && (
        <Chip onClick={() => setExpanded(true)} className="border-dashed bg-transparent">
          +{items.length - collapsedLimit}
        </Chip>
      )}
    </FadeKey>
  )
}
