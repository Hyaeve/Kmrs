import { useCallback, useEffect, useMemo, useState } from 'react'
import { keepPreviousData, useInfiniteQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { BookOpen, WarningCircle } from '@phosphor-icons/react'
import { booksApi } from '@/lib/api/books'
import { Button } from '@/components/ui/Button'
import { EmptyState } from '@/components/ui/EmptyState'
import { CardSkeleton, GridSkeleton } from '@/components/ui/Skeleton'
import { MediaGrid } from '@/components/media/MediaGrid'
import { BookCard } from '@/components/media/BookCard'
import { FilterBar } from '@/components/filters/FilterBar'
import { FilterDrawer } from '@/components/filters/FilterDrawer'
import { Sentinel } from '@/components/filters/Sentinel'
import { activeFilterCount, serializeFilters, useBrowseFilters } from '@/components/filters/filterUrl'
import { serializeSort, useSortState } from '@/components/filters/sort'
import { buildBookSearch } from '@/components/filters/builders'
import { BOOK_DEFAULT_SORT, BOOK_FILTER_GROUPS, BOOK_SORT_OPTIONS } from '@/components/filters/types'
import { cardSelection, useSelection } from '@/components/selection/useSelection'
import { BooksSelectionBar } from '@/components/browse/BooksSelectionBar'

/** Book filter bar and card grid with selection, shared by the global books page and the library tab. */
export function BooksGrid({ libraryId }: { libraryId?: string }) {
  const { t } = useTranslation('browse')
  const filters = useBrowseFilters(`books:${libraryId ?? 'all'}`)
  const sort = useSortState(`books:${libraryId ?? 'all'}`, BOOK_DEFAULT_SORT)
  const [drawerOpen, setDrawerOpen] = useState(false)
  const selection = useSelection()

  const search = useMemo(() => buildBookSearch(filters.state, libraryId), [filters.state, libraryId])
  const sortParam = serializeSort(sort.current)
  const filterKey = serializeFilters(filters.state)

  const q = useInfiniteQuery({
    queryKey: ['books', 'list', libraryId ?? 'all', filterKey, sortParam],
    queryFn: ({ pageParam }) => booksApi.list({ search, page: pageParam, size: 50, sort: [sortParam] }),
    getNextPageParam: (last) => (last.last ? undefined : last.number + 1),
    initialPageParam: 0,
    placeholderData: keepPreviousData,
  })

  const items = useMemo(() => q.data?.pages.flatMap((p) => p.content) ?? [], [q.data?.pages])
  const loadedIds = useMemo(() => items.map((b) => b.id), [items])
  const total = q.data?.pages[0]?.totalElements
  const { hasNextPage, isFetchingNextPage, isPlaceholderData, fetchNextPage } = q
  const loadMore = useCallback(() => {
    // isPlaceholderData means a stale query is shown while filters changed; don't page the old one
    if (hasNextPage && !isFetchingNextPage && !isPlaceholderData) void fetchNextPage()
  }, [hasNextPage, isFetchingNextPage, isPlaceholderData, fetchNextPage])

  // a changed result set invalidates any selection made against the old one
  const clearSelection = selection.clear
  useEffect(() => {
    clearSelection()
  }, [filterKey, sortParam, libraryId, clearSelection])

  return (
    <>
      <FilterBar
        count={total}
        noun="books"
        groups={BOOK_FILTER_GROUPS}
        state={filters.state}
        activeCount={activeFilterCount(filters.state)}
        onToggleValue={filters.toggleValue}
        onToggleAuthor={filters.toggleAuthor}
        onQChange={filters.setQ}
        onOpenFilters={() => setDrawerOpen(true)}
        sortOptions={BOOK_SORT_OPTIONS}
        sort={sort.current}
        onSortChange={sort.set}
      />
      {q.isPending ? (
        <GridSkeleton count={18} />
      ) : q.isLoadingError ? (
        <EmptyState
          icon={<WarningCircle />}
          title={t('books.loadError')}
          body={q.error?.message || t('errorFallback')}
          action={<Button onClick={() => q.refetch()}>{t('common:action.retry')}</Button>}
        />
      ) : items.length === 0 ? (
        <EmptyState icon={<BookOpen />} title={t('books.empty')} body={t('emptyFilterHint')} />
      ) : (
        <>
          <MediaGrid>
            {items.map((b) => (
              <BookCard key={b.id} book={b} showSeries selection={cardSelection(selection, b.id, loadedIds)} />
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
      <FilterDrawer
        open={drawerOpen}
        onClose={() => setDrawerOpen(false)}
        groups={BOOK_FILTER_GROUPS}
        state={filters.state}
        scope={{ libraryId: libraryId ? [libraryId] : undefined }}
        activeCount={activeFilterCount(filters.state)}
        onToggleValue={filters.toggleValue}
        onToggleAuthor={filters.toggleAuthor}
        onSetMode={filters.setMode}
        onSetNegated={filters.setNegated}
        onSetExclusive={filters.setExclusive}
        onCycleValue={filters.cycleValue}
        onClearGroup={filters.clearGroup}
        onClearAll={filters.clearAll}
      />
      <BooksSelectionBar selection={selection} loadedIds={loadedIds} />
    </>
  )
}
