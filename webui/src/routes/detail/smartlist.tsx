import { useCallback, useMemo, useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { keepPreviousData, useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { BookOpen, CircleNotch, DotsThreeVertical, EyeSlash, Image, PencilSimple, Trash } from '@phosphor-icons/react'
import { smartListsApi } from '@/lib/api/smartLists'
import type { BookDto, BookSearch, Page, SeriesDto, SeriesSearch, SmartListDto } from '@/lib/api/types'
import { isAdmin, isOwner, useAuthStore } from '@/lib/store/auth'
import { useDocumentTitle } from '@/lib/hooks/useDocumentTitle'
import { readRoute } from '@/lib/utils/nav'
import { Button } from '@/components/ui/Button'
import { BackButton } from '@/components/ui/BackButton'
import { Tooltip } from '@/components/ui/Tooltip'
import { IconButton } from '@/components/ui/IconButton'
import { Menu, MenuItem, MenuSeparator } from '@/components/ui/Menu'
import { EmptyState } from '@/components/ui/EmptyState'
import { PageHeader } from '@/components/ui/PageHeader'
import { Skeleton, GridSkeleton } from '@/components/ui/Skeleton'
import { MediaGrid } from '@/components/media/MediaGrid'
import { BookCard } from '@/components/media/BookCard'
import { SeriesCard } from '@/components/media/SeriesCard'
import { DetailError } from '@/components/detail/DetailError'
import { ConfirmDeleteDialog } from '@/components/detail/ConfirmDeleteDialog'
import { PosterManager } from '@/components/metadata/PosterManager'
import { FilterBar } from '@/components/filters/FilterBar'
import { FilterDrawer } from '@/components/filters/FilterDrawer'
import { Sentinel } from '@/components/filters/Sentinel'
import { activeFilterCount, serializeFilters, useBrowseFilters } from '@/components/filters/filterUrl'
import { serializeSort, useSortState } from '@/components/filters/sort'
import { buildBookSearch, buildSeriesSearch } from '@/components/filters/builders'
import {
  BOOK_DEFAULT_SORT,
  BOOK_SORT_OPTIONS,
  SERIES_DEFAULT_SORT,
  SERIES_SORT_OPTIONS,
  BOOK_FILTER_GROUPS,
  SERIES_FILTER_GROUPS,
} from '@/components/filters/types'
import { SmartListDialog } from '@/components/smartlists/SmartListDialog'

const PAGE_SIZE = 48

// module-level so the hook's memo/effect deps stay stable across renders
const NO_DISABLED_FILTERS = [] as const

function SmartListContent({ list, isBook }: { list: SmartListDto; isBook: boolean }) {
  const { t } = useTranslation('smartlists')
  const queryClient = useQueryClient()
  const user = useAuthStore((s) => s.user)
  const navigate = useNavigate()
  const [editOpen, setEditOpen] = useState(false)
  const [deleteOpen, setDeleteOpen] = useState(false)
  const [postersOpen, setPostersOpen] = useState(false)

  const filters = useBrowseFilters(NO_DISABLED_FILTERS)
  const [drawerOpen, setDrawerOpen] = useState(false)
  const hasFilters = activeFilterCount(filters.state) > 0 || !!filters.state.q.trim()

  // page-side filters are ANDed with the stored filter by the server (overlay body)
  const overlay = useMemo(
    () => (isBook ? buildBookSearch(filters.state) : buildSeriesSearch(filters.state)),
    [filters.state, isBook],
  )
  const sort = useSortState(isBook ? 'books:smartlist' : 'series:smartlist', isBook ? BOOK_DEFAULT_SORT : SERIES_DEFAULT_SORT)
  const sortParam = serializeSort(sort.current)
  const filterKey = serializeFilters(filters.state)
  const admin = isAdmin(user)
  const groups = useMemo(
    () => (isBook ? BOOK_FILTER_GROUPS : SERIES_FILTER_GROUPS).filter((g) => !g.adminOnly || admin),
    [isBook, admin],
  )

  const itemsQuery = useInfiniteQuery({
    queryKey: ['smart-lists', list.id, 'items', filterKey, sortParam],
    queryFn: ({ pageParam }): Promise<Page<BookDto | SeriesDto>> => {
      const params = { page: pageParam, size: PAGE_SIZE, sort: [sortParam] }
      return isBook
        ? smartListsApi.books(list.id, params, overlay as BookSearch)
        : smartListsApi.series(list.id, params, overlay as SeriesSearch)
    },
    initialPageParam: 0,
    getNextPageParam: (last) => (last.last ? undefined : last.number + 1),
    placeholderData: keepPreviousData,
  })

  // first unread book of the evaluated set, for the read/peek entry points
  const continueQuery = useQuery({
    queryKey: ['smart-lists', list.id, 'continue-target'],
    queryFn: async () => {
      const unread = await smartListsApi.books(list.id, { size: 1 }, { condition: { readStatus: { operator: 'is', value: 'UNREAD' } } })
      if (unread.content[0]) return unread.content[0]
      const first = await smartListsApi.books(list.id, { size: 1 }, {})
      return first.content[0] ?? null
    },
    enabled: isBook,
  })

  const deleteMutation = useMutation({
    mutationFn: () => smartListsApi.delete(list.id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['smart-lists'] })
      navigate('/smart-lists')
    },
  })

  const { hasNextPage, isFetchingNextPage, isPlaceholderData, fetchNextPage } = itemsQuery
  const loadMore = useCallback(() => {
    if (hasNextPage && !isFetchingNextPage && !isPlaceholderData) void fetchNextPage()
  }, [hasNextPage, isFetchingNextPage, isPlaceholderData, fetchNextPage])

  const items = itemsQuery.data?.pages.flatMap((p) => p.content) ?? []
  const total = itemsQuery.data?.pages[0]?.totalElements
  // edit/delete are owner-or-admin, matching the API; cover uploads stay owner-only there
  const own = isOwner(user, list.ownerId)
  const canManage = own || admin
  const continueTarget = continueQuery.data
  const continueRoute = continueTarget ? readRoute(continueTarget) : null

  return (
    <div>
      <PageHeader
        title={list.name}
        subtitle={total !== undefined ? t(`count.${isBook ? 'book' : 'series'}`, { count: total }) : undefined}
        actions={
          <>
            {isBook && (
              <>
                <Button
                  variant="primary"
                  size="sm"
                  disabled={!continueRoute}
                  loading={continueQuery.isPending}
                  onClick={() => continueRoute && navigate(continueRoute)}
                >
                  <BookOpen className="size-4" /> {t('read')}
                </Button>
                <Tooltip content={t('peekTooltip')}>
                  <Button
                    variant="secondary"
                    size="sm"
                    disabled={!continueRoute}
                    onClick={() => continueRoute && navigate(`${continueRoute}?incognito=true`)}
                  >
                    <EyeSlash className="size-4" /> {t('peek')}
                  </Button>
                </Tooltip>
              </>
            )}
            {canManage && (
              <Menu
                trigger={
                  <IconButton label={t('moreActions')} className="size-8">
                    <DotsThreeVertical className="size-5" />
                  </IconButton>
                }
              >
                {own && (
                  <MenuItem onSelect={() => setPostersOpen(true)}>
                    <Image className="size-4" /> {t('managePosters')}
                  </MenuItem>
                )}
                <MenuItem onSelect={() => setEditOpen(true)}>
                  <PencilSimple className="size-4" /> {t('common:action.edit')}
                </MenuItem>
                <MenuSeparator />
                <MenuItem danger onSelect={() => setDeleteOpen(true)}>
                  <Trash className="size-4" /> {t('common:action.delete')}
                </MenuItem>
              </Menu>
            )}
          </>
        }
      />

      {list.summary && <p className="-mt-3 mb-6 max-w-3xl text-sm text-ink-2">{list.summary}</p>}

      <FilterBar
        count={total}
        noun={isBook ? 'books' : 'series'}
        groups={groups}
        state={filters.state}
        activeCount={activeFilterCount(filters.state)}
        onToggleValue={filters.toggleValue}
        onToggleAuthor={filters.toggleAuthor}
        onQChange={filters.setQ}
        onOpenFilters={() => setDrawerOpen(true)}
        sortOptions={isBook ? BOOK_SORT_OPTIONS : SERIES_SORT_OPTIONS}
        sort={sort.current}
        onSortChange={sort.set}
      />

      {itemsQuery.isPending ? (
        <GridSkeleton />
      ) : itemsQuery.isLoadingError ? (
        <EmptyState
          title={t('loadItemsError')}
          body={itemsQuery.error.message}
          action={
            <Button variant="secondary" onClick={() => itemsQuery.refetch()}>
              {t('common:action.retry')}
            </Button>
          }
        />
      ) : items.length === 0 ? (
        <EmptyState
          title={t(`emptyState.${isBook ? 'books' : 'series'}`)}
          body={hasFilters ? t('emptyState.filtered') : undefined}
        />
      ) : (
        <>
          <MediaGrid>
            {items.map((item) =>
              isBook ? (
                <BookCard key={item.id} book={item as BookDto} showSeries />
              ) : (
                <SeriesCard key={item.id} series={item as SeriesDto} />
              ),
            )}
          </MediaGrid>
          <Sentinel active={!!hasNextPage && !isPlaceholderData} onIntersect={loadMore} />
          {isFetchingNextPage && (
            <div className="mt-6 flex justify-center">
              <CircleNotch className="size-5 animate-spin text-ink-3" />
            </div>
          )}
        </>
      )}

      <FilterDrawer
        open={drawerOpen}
        onClose={() => setDrawerOpen(false)}
        groups={groups}
        state={filters.state}
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

      <SmartListDialog open={editOpen} onOpenChange={setEditOpen} existing={list} />
      <PosterManager
        open={postersOpen}
        onClose={() => setPostersOpen(false)}
        kind="smartlist"
        entityId={list.id}
        title={list.name}
      />
      <ConfirmDeleteDialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        title={t('deleteTitle')}
        name={list.name}
        loading={deleteMutation.isPending}
        onConfirm={() => deleteMutation.mutate()}
      />
    </div>
  )
}

export function SmartListDetailPage() {
  const { t } = useTranslation('smartlists')
  const { smartListId = '' } = useParams()

  const listQuery = useQuery({
    queryKey: ['smart-lists', smartListId],
    queryFn: () => smartListsApi.get(smartListId),
  })
  const list = listQuery.data

  useDocumentTitle(list?.name)

  if (listQuery.isPending)
    return (
      <div>
        <Skeleton className="h-9 w-64" />
        <Skeleton className="mt-2 h-4 w-20" />
        <GridSkeleton className="mt-8" />
      </div>
    )
  if (listQuery.isLoadingError)
    return (
      <DetailError error={listQuery.error} notFoundTitle={t('notFound')} onRetry={() => listQuery.refetch()} />
    )
  if (!list) return null

  return (
    <div>
      <BackButton to="/smart-lists" className="mb-2 -ml-2" />
      <SmartListContent key={`${list.id}-${list.lastModifiedDate}`} list={list} isBook={list.target === 'BOOK'} />
    </div>
  )
}
