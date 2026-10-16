import { useState } from 'react'
import { keepPreviousData, useInfiniteQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { ImageBroken, WarningCircle } from '@phosphor-icons/react'
import { booksApi } from '@/lib/api/books'
import type { BookSearch, SearchCondition } from '@/lib/api/types'
import { useDocumentTitle } from '@/lib/hooks/useDocumentTitle'
import { Button } from '@/components/ui/Button'
import { EmptyState } from '@/components/ui/EmptyState'
import { PageHeader } from '@/components/ui/PageHeader'
import { CardSkeleton, GridSkeleton } from '@/components/ui/Skeleton'
import { MediaGrid } from '@/components/media/MediaGrid'
import { BookCard } from '@/components/media/BookCard'
import { LibraryFilterMenu } from '@/components/admin/LibraryFilterMenu'
import { Sentinel } from '@/components/filters/Sentinel'

function searchFor(libraryId: string | null): BookSearch {
  const conditions: SearchCondition[] = [
    { deleted: { operator: 'isFalse' } },
    // no poster is marked selected: the book falls back to a generated cover
    { poster: { operator: 'isNot', value: { selected: true } } },
  ]
  if (libraryId) conditions.unshift({ libraryId: { operator: 'is', value: libraryId } })
  return { condition: { allOf: conditions } }
}

export function AdminMissingPostersPage() {
  const { t } = useTranslation('admin-maintenance')
  const [libraryId, setLibraryId] = useState<string | null>(null)

  useDocumentTitle(t('layout:nav.missingPosters'))

  const q = useInfiniteQuery({
    queryKey: ['admin', 'missing-posters', libraryId ?? 'all'],
    queryFn: ({ pageParam }) =>
      booksApi.list({ search: searchFor(libraryId), page: pageParam, size: 50, sort: ['series,asc'] }),
    getNextPageParam: (last) => (last.last ? undefined : last.number + 1),
    initialPageParam: 0,
    placeholderData: keepPreviousData,
  })

  const items = q.data?.pages.flatMap((p) => p.content) ?? []
  const total = q.data?.pages[0]?.totalElements

  return (
    <div>
      <PageHeader
        title={t('layout:nav.missingPosters')}
        subtitle={t('missingPosters.subtitle')}
        actions={<LibraryFilterMenu value={libraryId} onChange={setLibraryId} />}
      />

      {q.isPending ? (
        <GridSkeleton count={18} />
      ) : q.isLoadingError ? (
        <EmptyState
          icon={<WarningCircle />}
          title={t('browse:books.loadError')}
          body={q.error instanceof Error ? q.error.message : t('errorFallback')}
          action={<Button onClick={() => q.refetch()}>{t('common:action.retry')}</Button>}
        />
      ) : items.length === 0 ? (
        <EmptyState
          icon={<ImageBroken />}
          title={t('missingPosters.emptyTitle')}
          body={t('missingPosters.emptyBody')}
        />
      ) : (
        <>
          <p className="mb-4 text-sm text-ink-3">{t('missingPosters.count', { count: total ?? items.length })}</p>
          <MediaGrid>
            {items.map((b) => (
              <BookCard key={b.id} book={b} showSeries />
            ))}
          </MediaGrid>
          {q.isFetchingNextPage && (
            <MediaGrid className="mt-7">
              {Array.from({ length: 4 }, (_, i) => (
                <CardSkeleton key={i} />
              ))}
            </MediaGrid>
          )}
          <Sentinel
            active={!!q.hasNextPage && !q.isPlaceholderData}
            onIntersect={() => {
              if (q.hasNextPage && !q.isFetchingNextPage) void q.fetchNextPage()
            }}
          />
        </>
      )}
    </div>
  )
}
