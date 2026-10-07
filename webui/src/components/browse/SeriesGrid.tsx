import { useCallback, useEffect, useMemo, useState } from 'react'
import { keepPreviousData, useInfiniteQuery, useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Books, WarningCircle } from '@phosphor-icons/react'
import { seriesApi } from '@/lib/api/series'
import { isAdmin, useAuthStore } from '@/lib/store/auth'
import { Button } from '@/components/ui/Button'
import { EmptyState } from '@/components/ui/EmptyState'
import { CardSkeleton, GridSkeleton } from '@/components/ui/Skeleton'
import { MediaGrid } from '@/components/media/MediaGrid'
import { SeriesCard } from '@/components/media/SeriesCard'
import { FilterBar } from '@/components/filters/FilterBar'
import { FilterDrawer } from '@/components/filters/FilterDrawer'
import { Sentinel } from '@/components/filters/Sentinel'
import { activeFilterCount, serializeFilters, useBrowseFilters } from '@/components/filters/filterUrl'
import { serializeSort, useSortState } from '@/components/filters/sort'
import { buildSeriesSearch } from '@/components/filters/builders'
import { SERIES_DEFAULT_SORT, SERIES_FILTER_GROUPS, SERIES_SORT_OPTIONS } from '@/components/filters/types'
import { cardSelection, useSelection } from '@/components/selection/useSelection'
import { AlphabetBar } from '@/components/browse/AlphabetBar'
import { SeriesSelectionBar } from '@/components/browse/SeriesSelectionBar'

/** Series filter bar, alphabet bar and card grid with selection, shared by the
    global series page and the library tab. */
export function SeriesGrid({ libraryId }: { libraryId?: string }) {
  const { t } = useTranslation('browse')
  const filters = useBrowseFilters()
  const sort = useSortState(`series:${libraryId ?? 'all'}`, SERIES_DEFAULT_SORT)
  const [drawerOpen, setDrawerOpen] = useState(false)
  const admin = isAdmin(useAuthStore((s) => s.user))
  const groups = useMemo(() => SERIES_FILTER_GROUPS.filter((g) => !g.adminOnly || admin), [admin])
  const selection = useSelection()

  const search = useMemo(() => buildSeriesSearch(filters.state, libraryId), [filters.state, libraryId])
  const sortParam = serializeSort(sort.current)
  const filterKey = serializeFilters(filters.state)

  // the letter filter must not collapse the alphabet bar itself, so groups are counted without it
  const alphaSearch = useMemo(
    () => buildSeriesSearch({ ...filters.state, letter: [] }, libraryId),
    [filters.state, libraryId],
  )
  const alphaKey = serializeFilters({ ...filters.state, letter: [] })
  const alphaQuery = useQuery({
    queryKey: ['series', 'alphabet', libraryId ?? 'all', alphaKey],
    queryFn: () => seriesApi.alphabeticalGroups(alphaSearch),
    placeholderData: keepPreviousData,
  })

  const q = useInfiniteQuery({
    queryKey: ['series', 'list', libraryId ?? 'all', filterKey, sortParam],
    queryFn: ({ pageParam }) => seriesApi.list({ search, page: pageParam, size: 50, sort: [sortParam] }),
    getNextPageParam: (last) => (last.last ? undefined : last.number + 1),
    initialPageParam: 0,
    placeholderData: keepPreviousData,
  })

  const items = useMemo(() => q.data?.pages.flatMap((p) => p.content) ?? [], [q.data?.pages])
  const loadedIds = useMemo(() => items.map((s) => s.id), [items])
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
        noun="series"
        groups={groups}
        state={filters.state}
        activeCount={activeFilterCount(filters.state)}
        onToggleValue={filters.toggleValue}
        onToggleAuthor={filters.toggleAuthor}
        onQChange={filters.setQ}
        onOpenFilters={() => setDrawerOpen(true)}
        sortOptions={SERIES_SORT_OPTIONS}
        sort={sort.current}
        onSortChange={sort.set}
      />
      <AlphabetBar
        groups={alphaQuery.data}
        active={filters.state.letter[0]}
        onSelect={(l) => filters.setExclusive('letter', l)}
      />
      {q.isPending ? (
        <GridSkeleton count={18} />
      ) : q.isLoadingError ? (
        <EmptyState
          icon={<WarningCircle />}
          title={t('series.loadError')}
          body={q.error?.message || t('errorFallback')}
          action={<Button onClick={() => q.refetch()}>{t('common:action.retry')}</Button>}
        />
      ) : items.length === 0 ? (
        <EmptyState icon={<Books />} title={t('series.empty')} body={t('emptyFilterHint')} />
      ) : (
        <>
          <MediaGrid>
            {items.map((s) => (
              <SeriesCard key={s.id} series={s} selection={cardSelection(selection, s.id, loadedIds)} />
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
        groups={groups}
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
      <SeriesSelectionBar selection={selection} items={items} />
    </>
  )
}
