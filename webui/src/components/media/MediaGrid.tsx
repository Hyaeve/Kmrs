import type { ReactNode } from 'react'
import { cn } from '@/lib/utils/cn'
import { densityScale, useUiStore } from '@/lib/store/ui'

interface MediaGridProps {
  children: ReactNode
  className?: string
  /** base card width in px before the density multiplier */
  baseWidth?: number
}

export function MediaGrid({ children, className, baseWidth = 140 }: MediaGridProps) {
  const density = useUiStore((s) => s.gridDensity)
  const min = Math.round(baseWidth * densityScale(density))
  return (
    <div
      className={cn('grid gap-x-4 gap-y-7', className)}
      style={{ gridTemplateColumns: `repeat(auto-fill, minmax(${min}px, 1fr))` }}
    >
      {children}
    </div>
  )
}
