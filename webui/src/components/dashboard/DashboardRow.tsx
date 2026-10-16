import { Fragment, type ReactNode } from 'react'
import type { InfiniteData, UseInfiniteQueryResult } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { WarningCircle } from '@phosphor-icons/react'
import type { Page } from '@/lib/api/types'
import { HorizontalRow } from '@/components/media/HorizontalRow'
import { CardSkeleton } from '@/components/ui/Skeleton'
import { Button } from '@/components/ui/Button'
import { useDensityCardWidth } from '@/lib/store/ui'

export type PagedRowQuery<T> = UseInfiniteQueryResult<InfiniteData<Page<T>, number>> & { items: T[] }

interface DashboardRowProps<T> {
  title: string
  to: string
  query: PagedRowQuery<T>
  keyOf: (item: T) => string
  renderItem: (item: T) => ReactNode
  /** pending placeholder item; defaults to grid card skeletons */
  skeleton?: ReactNode
}

export function DashboardRow<T>({ title, to, query, keyOf, renderItem, skeleton }: DashboardRowProps<T>) {
  const { t } = useTranslation('dashboard')
  const cardWidth = useDensityCardWidth()
  if (query.isPending) {
    return (
      <HorizontalRow title={title} to={to}>
        {skeleton
          ? Array.from({ length: 3 }, (_, i) => <Fragment key={i}>{skeleton}</Fragment>)
          : Array.from({ length: 5 }, (_, i) => (
              <div key={i} className="shrink-0 snap-start" style={{ width: cardWidth }}>
                <CardSkeleton />
              </div>
            ))}
      </HorizontalRow>
    )
  }

  if (query.isLoadingError) {
    return (
      <HorizontalRow title={title} to={to}>
        <div
          role="alert"
          className="flex w-full shrink-0 items-center gap-3 rounded-xl border border-line bg-surface px-4 py-5"
        >
          <WarningCircle className="size-5 shrink-0 text-danger" />
          <p className="min-w-0 flex-1 text-sm text-ink-2">{t('rowError')}</p>
          <Button size="sm" loading={query.isRefetching} onClick={() => void query.refetch()}>
            {t('common:action.retry')}
          </Button>
        </div>
      </HorizontalRow>
    )
  }

  if (query.items.length === 0) return null

  return (
    <HorizontalRow
      title={title}
      to={to}
      onEndReached={() => {
        if (query.hasNextPage && !query.isFetchingNextPage) void query.fetchNextPage()
      }}
    >
      {query.items.map((item) => (
        <Fragment key={keyOf(item)}>{renderItem(item)}</Fragment>
      ))}
    </HorizontalRow>
  )
}
