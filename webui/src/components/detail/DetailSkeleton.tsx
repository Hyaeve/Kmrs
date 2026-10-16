import { Skeleton } from '@/components/ui/Skeleton'

export function DetailSkeleton() {
  return (
    <div>
      <div className="flex gap-6">
        <Skeleton className="cover-aspect w-36 shrink-0 md:w-44" />
        <div className="min-w-0 flex-1 space-y-3 py-1">
          <Skeleton className="h-3 w-24" />
          <Skeleton className="h-10 w-3/4 max-w-md" />
          <Skeleton className="h-4 w-1/3" />
          <div className="flex gap-2 pt-3">
            <Skeleton className="h-10 w-24" />
            <Skeleton className="h-10 w-32" />
            <Skeleton className="h-10 w-28" />
          </div>
        </div>
      </div>
      <div className="mt-6 space-y-2">
        <Skeleton className="h-4 w-full max-w-3xl" />
        <Skeleton className="h-4 w-5/6 max-w-2xl" />
        <Skeleton className="h-4 w-2/3 max-w-xl" />
      </div>
    </div>
  )
}
