import { cn } from '@/lib/utils/cn'
import { useDensityCardWidth } from '@/lib/store/ui'

export function Skeleton({ className }: { className?: string }) {
  return <div className={cn('shimmer rounded-lg', className)} />
}

export function CardSkeleton({ className }: { className?: string }) {
  return (
    <div className={cn('flex flex-col gap-2', className)}>
      <Skeleton className="cover-aspect w-full rounded-lg" />
      <Skeleton className="h-3.5 w-4/5" />
      <Skeleton className="h-3 w-3/5" />
    </div>
  )
}

export function HorizontalCardSkeleton() {
  return (
    <div className="flex w-[250px] shrink-0 snap-start items-center gap-3 rounded-xl bg-raised p-2">
      <Skeleton className="cover-aspect w-[45px] shrink-0 rounded-md" />
      <div className="flex min-w-0 flex-1 flex-col gap-1.5 py-1">
        <Skeleton className="h-3.5 w-4/5" />
        <Skeleton className="h-3 w-3/5" />
        <Skeleton className="mt-auto h-3 w-2/5" />
      </div>
    </div>
  )
}

export function GridSkeleton({ count = 12, className }: { count?: number; className?: string }) {
  const min = useDensityCardWidth()
  return (
    <div className={cn('grid gap-x-4 gap-y-6', className)} style={{ gridTemplateColumns: `repeat(auto-fill, minmax(${min}px, 1fr))` }}>
      {Array.from({ length: count }, (_, i) => (
        <CardSkeleton key={i} />
      ))}
    </div>
  )
}
