import { useCallback, useEffect, useState } from 'react'
import { useSearchParams } from 'react-router-dom'
import { keepPreviousData, useInfiniteQuery } from '@tanstack/react-query'
import { Trans, useTranslation } from 'react-i18next'
import { BookmarkSimple, MagnifyingGlass, WarningCircle } from '@phosphor-icons/react'
import { readlistsApi } from '@/lib/api/collections'
import { useChanged } from '@/lib/hooks/useChanged'
import { Button } from '@/components/ui/Button'
import { EmptyState } from '@/components/ui/EmptyState'
import { CardSkeleton, GridSkeleton } from '@/components/ui/Skeleton'
import { MediaGrid } from '@/components/media/MediaGrid'
import { ReadListCard } from '@/components/media/SeriesCard'
import { Sentinel } from '@/components/filters/Sentinel'

/** Read list count + card grid, shared by the global read lists page and the library tab.
    Text search is offered only on the global page (`searchable`). */
export function ReadListsGrid({ libraryId, searchable }: { libraryId?: string; searchable?: boolean }) {
  const { t } = useTranslation('browse')
  const [searchParams, setSearchParams] = useSearchParams()
  const qParam = searchable ? (searchParams.get('q') ?? '') : ''
  const [text, setText] = useState(qParam)

  // external URL changes (back/forward, shared links) sync back into the input
  if (useChanged([qParam, searchable]) && searchable) setText(qParam)

  useEffect(() => {
    if (!searchable || text === qParam) return
    const t = setTimeout(() => {
      const next = new URLSearchParams(searchParams)
      if (text.trim()) next.set('q', text)
      else next.delete('q')
      setSearchParams(next, { replace: true })
    }, 300)
    return () => clearTimeout(t)
  }, [text, qParam, searchParams, setSearchParams, searchable])

  const q = useInfiniteQuery({
    queryKey: ['readlists', 'list', libraryId ?? 'all', qParam],
    queryFn: ({ pageParam }) =>
      readlistsApi.list({
        search: qParam || undefined,
        libraryId: libraryId ? [libraryId] : undefined,
        page: pageParam,
        size: 50,
      }),
    getNextPageParam: (last) => (last.last ? undefined : last.number + 1),
    initialPageParam: 0,
    placeholderData: keepPreviousData,
  })

  const items = q.data?.pages.flatMap((p) => p.content) ?? []
  const total = q.data?.pages[0]?.totalElements
  const { hasNextPage, isFetchingNextPage, isPlaceholderData, fetchNextPage } = q
  const loadMore = useCallback(() => {
    if (hasNextPage && !isFetchingNextPage && !isPlaceholderData) void fetchNextPage()
  }, [hasNextPage, isFetchingNextPage, isPlaceholderData, fetchNextPage])

  return (
    <>
      <div className="mb-5 flex min-h-8 flex-wrap items-center gap-x-3 gap-y-2">
        <span className="text-xs text-ink-3">
          {total !== undefined && (
            <Trans i18nKey="filters:noun.readlists" count={total} components={{ num: <span className="font-mono" /> }} />
          )}
        </span>
        {searchable && (
          <div className="flex min-w-0 flex-1 items-center justify-end">
            <div className="relative w-full max-w-xs">
              <MagnifyingGlass className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-ink-3" />
              <input
                type="search"
                value={text}
                onChange={(e) => setText(e.target.value)}
                placeholder={t('filters:search.readlists')}
                className="h-8 w-full rounded-lg border border-line bg-surface pr-3 pl-9 text-sm text-ink transition-colors placeholder:text-ink-3 focus:border-accent/70 focus:outline-none"
              />
            </div>
          </div>
        )}
      </div>
      {q.isPending ? (
        <GridSkeleton count={18} />
      ) : q.isLoadingError ? (
        <EmptyState
          icon={<WarningCircle />}
          title={t('readlists.loadError')}
          body={q.error?.message || t('errorFallback')}
          action={<Button onClick={() => q.refetch()}>{t('common:action.retry')}</Button>}
        />
      ) : items.length === 0 ? (
        <EmptyState icon={<BookmarkSimple />} title={t('readlists.empty')} body={qParam ? t('tryDifferentSearch') : undefined} />
      ) : (
        <>
          <MediaGrid>
            {items.map((r) => (
              <ReadListCard key={r.id} id={r.id} name={r.name} count={r.bookIds.length} />
            ))}
          </MediaGrid>
          {isFetchingNextPage && (
            <MediaGrid className="mt-7">
              {Array.from({ length: 4 }, (_, i) => (
                <CardSkeleton key={i} />
              ))}
            </MediaGrid>
          )}
          <Sentinel active={!!hasNextPage && !isPlaceholderData} onIntersect={loadMore} />
        </>
      )}
    </>
  )
}
