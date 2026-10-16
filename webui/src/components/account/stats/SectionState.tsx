import type { ReactNode } from 'react'
import type { UseQueryResult } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/Button'
import { Skeleton } from '@/components/ui/Skeleton'

/** Shared loading/error body for the stats sections, so each keeps only its data view. */
export function SectionState<T>({
  query,
  skeleton,
  children,
}: {
  query: UseQueryResult<T>
  skeleton?: ReactNode
  children: (data: T) => ReactNode
}) {
  const { t } = useTranslation('stats')
  if (query.isLoading) return <>{skeleton ?? <Skeleton className="h-24 w-full" />}</>
  if (query.isLoadingError) {
    return (
      <div className="flex items-center gap-3">
        <p className="text-sm text-danger">{query.error instanceof Error ? query.error.message : t('failed')}</p>
        <Button size="sm" onClick={() => void query.refetch()}>
          {t('common:action.retry')}
        </Button>
      </div>
    )
  }
  return query.data ? <>{children(query.data)}</> : null
}
