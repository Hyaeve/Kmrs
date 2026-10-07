import { keepPreviousData, useInfiniteQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { CheckCircle, WarningCircle } from '@phosphor-icons/react'
import { pageHashesApi } from '@/lib/api/pageHashes'
import type { PageHashAction, PageHashUnknownDto } from '@/lib/api/types'
import { Button } from '@/components/ui/Button'
import { EmptyState } from '@/components/ui/EmptyState'
import { Skeleton } from '@/components/ui/Skeleton'
import { Sentinel } from '@/components/filters/Sentinel'

function ReviewCard({ item }: { item: PageHashUnknownDto }) {
  const { t } = useTranslation('admin-maintenance')
  const queryClient = useQueryClient()

  const act = useMutation({
    mutationFn: (action: PageHashAction) =>
      pageHashesApi.createOrUpdate({ hash: item.hash, size: item.size, action }),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: ['admin', 'page-hashes'] }),
  })

  return (
    <div className="overflow-hidden rounded-xl border border-line bg-surface">
      <div
        aria-hidden
        className="h-44 w-full bg-raised bg-cover bg-center"
        style={{ backgroundImage: `url(${pageHashesApi.unknownThumbnailUrl(item.hash, 400)})` }}
      />
      <div className="flex flex-col gap-2.5 p-3">
        <div className="min-w-0">
          <code className="block truncate font-mono text-xs text-ink-2" title={item.hash}>
            {item.hash}
          </code>
          <p className="mt-0.5 text-xs text-ink-3">{t('pageHashes.occurrences', { count: item.matchCount })}</p>
        </div>
        <div className="flex flex-col gap-1.5">
          <Button size="sm" onClick={() => act.mutate('IGNORE')} disabled={act.isPending}>
            {t('pageHashes.action.ignore')}
          </Button>
          <Button size="sm" onClick={() => act.mutate('DELETE_MANUAL')} disabled={act.isPending}>
            {t('pageHashes.action.deleteManual')}
          </Button>
          <Button size="sm" variant="danger" onClick={() => act.mutate('DELETE_AUTO')} disabled={act.isPending}>
            {t('pageHashes.action.deleteAuto')}
          </Button>
        </div>
        {act.isError && (
          <p className="text-[11px] text-danger">
            {act.error instanceof Error ? act.error.message : t('pageHashes.saveActionFailed')}
          </p>
        )}
      </div>
    </div>
  )
}

export function UnknownTab() {
  const { t } = useTranslation('admin-maintenance')
  const q = useInfiniteQuery({
    queryKey: ['admin', 'page-hashes', 'unknown'],
    queryFn: ({ pageParam }) => pageHashesApi.listUnknown({ page: pageParam, size: 24 }),
    getNextPageParam: (last) => (last.last ? undefined : last.number + 1),
    initialPageParam: 0,
    placeholderData: keepPreviousData,
  })

  const items = q.data?.pages.flatMap((p) => p.content) ?? []
  const total = q.data?.pages[0]?.totalElements

  return (
    <div>
      <p className="mb-4 text-sm text-ink-3">
        {total !== undefined ? t('pageHashes.waitingReview', { count: total }) : t('pageHashes.waitingReviewFallback')}
        {' · '}
        {t('pageHashes.deleteAutoNote')}
      </p>

      {q.isPending ? (
        <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5">
          {Array.from({ length: 10 }, (_, i) => (
            <Skeleton key={i} className="h-72" />
          ))}
        </div>
      ) : q.isLoadingError ? (
        <EmptyState
          icon={<WarningCircle />}
          title={t('pageHashes.loadUnknownError')}
          body={q.error instanceof Error ? q.error.message : t('errorFallback')}
          action={<Button onClick={() => q.refetch()}>{t('common:action.retry')}</Button>}
        />
      ) : items.length === 0 ? (
        <EmptyState
          icon={<CheckCircle />}
          title={t('pageHashes.nothingToReview')}
          body={t('pageHashes.nothingToReviewBody')}
        />
      ) : (
        <>
          <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5">
            {items.map((item) => (
              <ReviewCard key={item.hash} item={item} />
            ))}
          </div>
          <Sentinel
            active={!!q.hasNextPage && !q.isPlaceholderData}
            onIntersect={() => {
              if (q.hasNextPage && !q.isFetchingNextPage) void q.fetchNextPage()
            }}
          />
          {q.isFetchingNextPage && <Skeleton className="mt-4 h-10 w-full" />}
        </>
      )}
    </div>
  )
}
