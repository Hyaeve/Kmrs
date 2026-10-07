import { useCallback, useMemo, useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { keepPreviousData, useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { CircleNotch, DotsThreeVertical, Image, ListChecks, PencilSimple, Plus, Trash } from '@phosphor-icons/react'
import { collectionsApi } from '@/lib/api/collections'
import { seriesApi } from '@/lib/api/series'
import type { CollectionDto } from '@/lib/api/types'
import { isAdmin, useAuthStore } from '@/lib/store/auth'
import { useDocumentTitle } from '@/lib/hooks/useDocumentTitle'
import { useChanged } from '@/lib/hooks/useChanged'
import { Button } from '@/components/ui/Button'
import { BackButton } from '@/components/ui/BackButton'
import { Dialog } from '@/components/ui/Dialog'
import { IconButton } from '@/components/ui/IconButton'
import { Menu, MenuItem, MenuSeparator } from '@/components/ui/Menu'
import { TextField } from '@/components/ui/TextField'
import { EmptyState } from '@/components/ui/EmptyState'
import { PageHeader } from '@/components/ui/PageHeader'
import { Skeleton, GridSkeleton } from '@/components/ui/Skeleton'
import { MediaGrid } from '@/components/media/MediaGrid'
import { SeriesCard } from '@/components/media/SeriesCard'
import { DetailError } from '@/components/detail/DetailError'
import { ConfirmDeleteDialog } from '@/components/detail/ConfirmDeleteDialog'
import { OrderBadge } from '@/components/detail/OrderBadge'
import { FilterBar } from '@/components/filters/FilterBar'
import { FilterDrawer } from '@/components/filters/FilterDrawer'
import { Sentinel } from '@/components/filters/Sentinel'
import { activeFilterCount, serializeFilters, useBrowseFilters } from '@/components/filters/filterUrl'
import { serializeSort, useSortState } from '@/components/filters/sort'
import { buildSeriesSearch } from '@/components/filters/builders'
import {
  COLLECTION_ORDER_SORT,
  COLLECTION_SERIES_SORT_OPTIONS,
  SERIES_DEFAULT_SORT,
  SERIES_FILTER_GROUPS,
} from '@/components/filters/types'
import { EditCollectionMembers } from '@/components/collections/EditCollectionMembers'
import { SeriesPickerDialog } from '@/components/collections/SeriesPickerDialog'
import { PosterManager } from '@/components/metadata/PosterManager'

const PAGE_SIZE = 48

// module-level so the hook's memo/effect deps stay stable across renders
const DISABLED_FILTERS = ['letter'] as const

function RenameCollectionDialog({
  open,
  onOpenChange,
  collection,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  collection: CollectionDto
}) {
  const [name, setName] = useState(collection.name)
  const queryClient = useQueryClient()
  const { t } = useTranslation('detail')

  if (useChanged([open, collection.name]) && open) setName(collection.name)

  const mutation = useMutation({
    mutationFn: () => collectionsApi.update(collection.id, { name: name.trim() }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['collections'] })
      onOpenChange(false)
    },
  })

  return (
    <Dialog open={open} onOpenChange={onOpenChange} title={t('collection.renameTitle')} size="sm">
      <form
        onSubmit={(e) => {
          e.preventDefault()
          if (name.trim()) mutation.mutate()
        }}
        className="px-5 py-4"
      >
        <TextField label={t('nameLabel')} value={name} onChange={(e) => setName(e.target.value)} autoFocus />
        <div className="mt-5 flex justify-end gap-2">
          <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
            {t('common:action.cancel')}
          </Button>
          <Button type="submit" variant="primary" loading={mutation.isPending} disabled={!name.trim()}>
            {t('common:action.save')}
          </Button>
        </div>
      </form>
    </Dialog>
  )
}

export function CollectionDetailPage() {
  const { t } = useTranslation('detail')
  const { collectionId = '' } = useParams()
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const admin = isAdmin(useAuthStore((s) => s.user))
  const [editing, setEditing] = useState(false)
  const [renameOpen, setRenameOpen] = useState(false)
  const [deleteOpen, setDeleteOpen] = useState(false)
  const [addSeriesOpen, setAddSeriesOpen] = useState(false)
  const [postersOpen, setPostersOpen] = useState(false)

  const filters = useBrowseFilters(DISABLED_FILTERS)
  const [drawerOpen, setDrawerOpen] = useState(false)
  const hasFilters = activeFilterCount(filters.state) > 0 || !!filters.state.q.trim()
  // first-letter navigation is a browse-page affordance, not useful inside a collection
  const groups = useMemo(() => SERIES_FILTER_GROUPS.filter((g) => g.key !== 'letter' && (!g.adminOnly || admin)), [admin])

  const collectionQuery = useQuery({
    queryKey: ['collections', collectionId],
    queryFn: () => collectionsApi.get(collectionId),
  })
  const collection = collectionQuery.data

  // ordered collections default to their manual order, unordered ones to title (kmrs behavior)
  const sort = useSortState('series:collection', collection?.ordered ? COLLECTION_ORDER_SORT : SERIES_DEFAULT_SORT)
  const search = useMemo(
    () => buildSeriesSearch(filters.state, undefined, { collectionId: { operator: 'is', value: collectionId } }),
    [filters.state, collectionId],
  )
  const sortParam = serializeSort(sort.current)
  const filterKey = serializeFilters(filters.state)

  const seriesQuery = useInfiniteQuery({
    queryKey: ['collections', collectionId, 'series', filterKey, sortParam],
    queryFn: ({ pageParam }) => seriesApi.list({ search, page: pageParam, size: PAGE_SIZE, sort: [sortParam] }),
    initialPageParam: 0,
    getNextPageParam: (last) => (last.last ? undefined : last.number + 1),
    placeholderData: keepPreviousData,
    enabled: !!collection && !editing,
  })

  const deleteMutation = useMutation({
    mutationFn: () => collectionsApi.delete(collectionId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['collections'] })
      navigate('/collections')
    },
  })

  const addSeriesMutation = useMutation({
    mutationFn: (ids: string[]) =>
      collectionsApi.update(collectionId, { seriesIds: [...(collection?.seriesIds ?? []), ...ids] }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['collections'] })
      setAddSeriesOpen(false)
    },
  })

  useDocumentTitle(collection?.name)

  const { hasNextPage, isFetchingNextPage, isPlaceholderData, fetchNextPage } = seriesQuery
  const loadMore = useCallback(() => {
    // isPlaceholderData means a stale query is shown while filters changed; don't page the old one
    if (hasNextPage && !isFetchingNextPage && !isPlaceholderData) void fetchNextPage()
  }, [hasNextPage, isFetchingNextPage, isPlaceholderData, fetchNextPage])

  if (collectionQuery.isPending)
    return (
      <div>
        <Skeleton className="h-9 w-64" />
        <Skeleton className="mt-2 h-4 w-20" />
        <GridSkeleton className="mt-8" />
      </div>
    )
  if (collectionQuery.isLoadingError)
    return (
      <DetailError
        error={collectionQuery.error}
        notFoundTitle={t('notFound.collection')}
        onRetry={() => collectionQuery.refetch()}
      />
    )
  if (!collection) return null

  const allSeries = seriesQuery.data?.pages.flatMap((p) => p.content) ?? []
  const seriesTotal = seriesQuery.data?.pages[0]?.totalElements
  // positions only match the badges when viewing the manual order top to bottom
  const showOrder = collection.ordered && sort.current.property === 'collection.number' && sort.current.direction === 'asc'

  return (
    <div>
      <BackButton to="/collections" className="mb-2 -ml-2" />
      <PageHeader
        title={collection.name}
        subtitle={t('seriesCount', { count: collection.seriesIds.length })}
        actions={
          admin &&
          !editing && (
            <>
              <Button variant="secondary" size="sm" onClick={() => setAddSeriesOpen(true)}>
                <Plus className="size-4" /> {t('addSeries')}
              </Button>
              <Button variant="secondary" size="sm" onClick={() => setEditing(true)}>
                <ListChecks className="size-4" /> {t('common:action.edit')}
              </Button>
              <Menu
                trigger={
                  <IconButton label={t('moreActions')} className="size-8">
                    <DotsThreeVertical className="size-5" />
                  </IconButton>
                }
              >
                <MenuItem onSelect={() => setPostersOpen(true)}>
                  <Image className="size-4" /> {t('menu.managePosters')}
                </MenuItem>
                <MenuItem onSelect={() => setRenameOpen(true)}>
                  <PencilSimple className="size-4" /> {t('rename')}
                </MenuItem>
                <MenuSeparator />
                <MenuItem danger onSelect={() => setDeleteOpen(true)}>
                  <Trash className="size-4" /> {t('common:action.delete')}
                </MenuItem>
              </Menu>
            </>
          )
        }
      />

      {editing ? (
        <EditCollectionMembers collection={collection} onExit={() => setEditing(false)} />
      ) : (
        <>
          <FilterBar
            count={seriesTotal}
            noun="series"
            groups={groups}
            state={filters.state}
            activeCount={activeFilterCount(filters.state)}
            onToggleValue={filters.toggleValue}
            onToggleAuthor={filters.toggleAuthor}
            onQChange={filters.setQ}
            onOpenFilters={() => setDrawerOpen(true)}
            sortOptions={COLLECTION_SERIES_SORT_OPTIONS}
            sort={sort.current}
            onSortChange={sort.set}
          />

          {seriesQuery.isPending ? (
            <GridSkeleton />
          ) : seriesQuery.isLoadingError ? (
            <EmptyState
              title={t('empty.loadSeriesFailed')}
              body={seriesQuery.error.message}
              action={
                <Button variant="secondary" onClick={() => seriesQuery.refetch()}>
                  {t('common:action.retry')}
                </Button>
              }
            />
          ) : allSeries.length === 0 ? (
            <EmptyState
              title={t('empty.noSeries')}
              body={hasFilters ? t('empty.noSeriesFiltered') : t('empty.collectionEmpty')}
            />
          ) : (
            <>
              <MediaGrid>
                {allSeries.map((s, i) => (
                  <div key={s.id} className="relative">
                    <SeriesCard series={s} />
                    {showOrder && <OrderBadge index={i + 1} />}
                  </div>
                ))}
              </MediaGrid>
              <Sentinel active={!!hasNextPage && !isPlaceholderData} onIntersect={loadMore} />
              {isFetchingNextPage && (
                <div className="mt-6 flex justify-center">
                  <CircleNotch className="size-5 animate-spin text-ink-3" />
                </div>
              )}
            </>
          )}
        </>
      )}

      <FilterDrawer
        open={drawerOpen}
        onClose={() => setDrawerOpen(false)}
        groups={groups}
        state={filters.state}
        scope={{ collectionId }}
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

      <RenameCollectionDialog open={renameOpen} onOpenChange={setRenameOpen} collection={collection} />
      <ConfirmDeleteDialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        title={t('collection.deleteTitle')}
        name={collection.name}
        loading={deleteMutation.isPending}
        onConfirm={() => deleteMutation.mutate()}
      />
      <SeriesPickerDialog
        open={addSeriesOpen}
        onOpenChange={setAddSeriesOpen}
        title={t('addSeries')}
        confirmLabel={t('addToCollection')}
        mode="multi"
        excludeIds={new Set(collection.seriesIds)}
        onConfirm={(selected) => addSeriesMutation.mutate(selected.map((s) => s.id))}
        confirming={addSeriesMutation.isPending}
        error={addSeriesMutation.error?.message}
      />
      <PosterManager
        open={postersOpen}
        onClose={() => setPostersOpen(false)}
        kind="collection"
        entityId={collection.id}
        title={collection.name}
      />
    </div>
  )
}
