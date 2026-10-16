import { useCallback } from 'react'
import { useInfiniteQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Funnel, WarningCircle } from '@phosphor-icons/react'
import { smartListsApi } from '@/lib/api/smartLists'
import { Button } from '@/components/ui/Button'
import { EmptyState } from '@/components/ui/EmptyState'
import { CardSkeleton, GridSkeleton } from '@/components/ui/Skeleton'
import { MediaGrid } from '@/components/media/MediaGrid'
import { SmartListCard } from '@/components/media/SmartListCard'
import { Sentinel } from '@/components/filters/Sentinel'

export function SmartListsGrid({ owner, onCreate }: { owner?: string; onCreate: () => void }) {
  const { t } = useTranslation('smartlists')

  const q = useInfiniteQuery({
    queryKey: ['smart-lists', 'list', owner ?? 'visible'],
    queryFn: ({ pageParam }) => smartListsApi.list({ page: pageParam, size: 50, owner }),
    getNextPageParam: (last) => (last.last ? undefined : last.number + 1),
    initialPageParam: 0,
  })

  const items = q.data?.pages.flatMap((p) => p.content) ?? []
  const { hasNextPage, isFetchingNextPage, fetchNextPage } = q
  const loadMore = useCallback(() => {
    if (hasNextPage && !isFetchingNextPage) void fetchNextPage()
  }, [hasNextPage, isFetchingNextPage, fetchNextPage])

  return (
    <>
      {q.isPending ? (
        <GridSkeleton count={18} />
      ) : q.isLoadingError ? (
        <EmptyState
          icon={<WarningCircle />}
          title={t('loadError')}
          body={q.error?.message}
          action={<Button onClick={() => q.refetch()}>{t('common:action.retry')}</Button>}
        />
      ) : items.length === 0 ? (
        <EmptyState icon={<Funnel />} title={t('empty')} action={<Button onClick={onCreate}>{t('create')}</Button>} />
      ) : (
        <>
          <MediaGrid>
            {items.map((l) => (
              <SmartListCard key={l.id} id={l.id} name={l.name} target={l.target} visibility={l.visibility} />
            ))}
          </MediaGrid>
          {isFetchingNextPage && (
            <MediaGrid className="mt-7">
              {Array.from({ length: 4 }, (_, i) => (
                <CardSkeleton key={i} />
              ))}
            </MediaGrid>
          )}
          <Sentinel active={!!hasNextPage} onIntersect={loadMore} />
        </>
      )}
    </>
  )
}
