import { useCallback, useMemo, useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { keepPreviousData, useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { BookOpen, CircleNotch, DotsThreeVertical, EyeSlash, Image, ListChecks, PencilSimple, Plus, Trash } from '@phosphor-icons/react'
import { readlistsApi } from '@/lib/api/collections'
import { booksApi } from '@/lib/api/books'
import type { ReadListDto } from '@/lib/api/types'
import { isAdmin, useAuthStore } from '@/lib/store/auth'
import { useDocumentTitle } from '@/lib/hooks/useDocumentTitle'
import { useChanged } from '@/lib/hooks/useChanged'
import { readRoute } from '@/lib/utils/nav'
import { Button } from '@/components/ui/Button'
import { BackButton } from '@/components/ui/BackButton'
import { Tooltip } from '@/components/ui/Tooltip'
import { Dialog } from '@/components/ui/Dialog'
import { IconButton } from '@/components/ui/IconButton'
import { Menu, MenuItem, MenuSeparator } from '@/components/ui/Menu'
import { TextField } from '@/components/ui/TextField'
import { EmptyState } from '@/components/ui/EmptyState'
import { PageHeader } from '@/components/ui/PageHeader'
import { Skeleton, GridSkeleton } from '@/components/ui/Skeleton'
import { MediaGrid } from '@/components/media/MediaGrid'
import { BookCard } from '@/components/media/BookCard'
import { DetailError } from '@/components/detail/DetailError'
import { ConfirmDeleteDialog } from '@/components/detail/ConfirmDeleteDialog'
import { OrderBadge } from '@/components/detail/OrderBadge'
import { FilterBar } from '@/components/filters/FilterBar'
import { FilterDrawer } from '@/components/filters/FilterDrawer'
import { Sentinel } from '@/components/filters/Sentinel'
import { activeFilterCount, serializeFilters, useBrowseFilters } from '@/components/filters/filterUrl'
import { serializeSort, useSortState } from '@/components/filters/sort'
import { buildBookSearch } from '@/components/filters/builders'
import { BOOK_DETAIL_FILTER_GROUPS, READLIST_BOOK_SORT_OPTIONS, READLIST_DATE_SORT, READLIST_ORDER_SORT } from '@/components/filters/types'
import { EditReadListBooks } from '@/components/readlists/EditReadListBooks'
import { BookPickerDialog } from '@/components/readlists/BookPickerDialog'
import { PosterManager } from '@/components/metadata/PosterManager'

const PAGE_SIZE = 48

// module-level so the hook's memo/effect deps stay stable across renders
const DISABLED_FILTERS = ['releaseYears'] as const

function EditReadListDialog({
  open,
  onOpenChange,
  readlist,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  readlist: ReadListDto
}) {
  const [name, setName] = useState(readlist.name)
  const [summary, setSummary] = useState(readlist.summary)
  const queryClient = useQueryClient()
  const { t } = useTranslation('detail')

  if (useChanged([open, readlist.name, readlist.summary]) && open) {
    setName(readlist.name)
    setSummary(readlist.summary)
  }

  const mutation = useMutation({
    mutationFn: () => readlistsApi.update(readlist.id, { name: name.trim(), summary: summary.trim() }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['readlists'] })
      onOpenChange(false)
    },
  })

  return (
    <Dialog open={open} onOpenChange={onOpenChange} title={t('readList.editTitle')} size="sm">
      <form
        onSubmit={(e) => {
          e.preventDefault()
          if (name.trim()) mutation.mutate()
        }}
        className="space-y-4 px-5 py-4"
      >
        <TextField label={t('nameLabel')} value={name} onChange={(e) => setName(e.target.value)} autoFocus />
        <div className="flex flex-col gap-2">
          <label htmlFor="readlist-summary" className="text-[13px] font-medium text-ink-2">
            {t('summaryLabel')}
          </label>
          <textarea
            id="readlist-summary"
            rows={3}
            value={summary}
            onChange={(e) => setSummary(e.target.value)}
            className="w-full rounded-lg border border-line bg-surface px-3 py-2 text-base text-ink transition-colors placeholder:text-ink-3 focus:border-accent/70 focus:outline-none"
          />
        </div>
        <div className="flex justify-end gap-2">
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

export function ReadListDetailPage() {
  const { t } = useTranslation('detail')
  const { readListId = '' } = useParams()
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const admin = isAdmin(useAuthStore((s) => s.user))
  const [editing, setEditing] = useState(false)
  const [editOpen, setEditOpen] = useState(false)
  const [deleteOpen, setDeleteOpen] = useState(false)
  const [addBooksOpen, setAddBooksOpen] = useState(false)
  const [postersOpen, setPostersOpen] = useState(false)

  const filters = useBrowseFilters('books:readlist', DISABLED_FILTERS)
  const [drawerOpen, setDrawerOpen] = useState(false)
  const hasFilters = activeFilterCount(filters.state) > 0 || !!filters.state.q.trim()

  const readlistQuery = useQuery({
    queryKey: ['readlists', readListId],
    queryFn: () => readlistsApi.get(readListId),
  })
  const readlist = readlistQuery.data

  // First unread (or in-progress) book, falling back to the first book when fully read
  const continueQuery = useQuery({
    queryKey: ['readlists', readListId, 'continue-target'],
    queryFn: async () => {
      const unread = await readlistsApi.books(readListId, { readStatus: ['UNREAD', 'IN_PROGRESS'], size: 1 })
      if (unread.content[0]) return unread.content[0]
      const first = await readlistsApi.books(readListId, { size: 1 })
      return first.content[0] ?? null
    },
    enabled: !!readlist,
  })
  // ordered lists default to their manual order, unordered ones to release date (kmrs behavior)
  const sort = useSortState('books:readlist', readlist?.ordered ? READLIST_ORDER_SORT : READLIST_DATE_SORT)
  const search = useMemo(
    () => buildBookSearch(filters.state, undefined, { readListId: { operator: 'is', value: readListId } }),
    [filters.state, readListId],
  )
  const sortParam = serializeSort(sort.current)
  const filterKey = serializeFilters(filters.state)

  const booksQuery = useInfiniteQuery({
    queryKey: ['readlists', readListId, 'books', filterKey, sortParam],
    queryFn: ({ pageParam }) => booksApi.list({ search, page: pageParam, size: PAGE_SIZE, sort: [sortParam] }),
    initialPageParam: 0,
    getNextPageParam: (last) => (last.last ? undefined : last.number + 1),
    placeholderData: keepPreviousData,
    enabled: !!readlist && !editing,
  })

  const deleteMutation = useMutation({
    mutationFn: () => readlistsApi.delete(readListId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['readlists'] })
      navigate('/readlists')
    },
  })

  const addBooksMutation = useMutation({
    mutationFn: (ids: string[]) => readlistsApi.update(readListId, { bookIds: [...(readlist?.bookIds ?? []), ...ids] }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['readlists'] })
      setAddBooksOpen(false)
    },
  })

  useDocumentTitle(readlist?.name)

  const { hasNextPage, isFetchingNextPage, isPlaceholderData, fetchNextPage } = booksQuery
  const loadMore = useCallback(() => {
    // isPlaceholderData means a stale query is shown while filters changed; don't page the old one
    if (hasNextPage && !isFetchingNextPage && !isPlaceholderData) void fetchNextPage()
  }, [hasNextPage, isFetchingNextPage, isPlaceholderData, fetchNextPage])

  if (readlistQuery.isPending)
    return (
      <div>
        <Skeleton className="h-9 w-64" />
        <Skeleton className="mt-2 h-4 w-20" />
        <GridSkeleton className="mt-8" />
      </div>
    )
  if (readlistQuery.isLoadingError)
    return (
      <DetailError error={readlistQuery.error} notFoundTitle={t('notFound.readList')} onRetry={() => readlistQuery.refetch()} />
    )
  if (!readlist) return null

  const books = booksQuery.data?.pages.flatMap((p) => p.content) ?? []
  const booksTotal = booksQuery.data?.pages[0]?.totalElements
  const continueTarget = continueQuery.data
  const continueRoute = continueTarget ? readRoute(continueTarget) : null
  // positions only match the badges when viewing the manual order top to bottom
  const showOrder = readlist.ordered && sort.current.property === 'readList.number' && sort.current.direction === 'asc'

  return (
    <div>
      <BackButton to="/readlists" className="mb-2 -ml-2" />
      <PageHeader
        title={readlist.name}
        subtitle={t('bookCount', { count: readlist.bookIds.length })}
        actions={
          <>
            {!editing && (
              <>
                <Button
                  variant="primary"
                  size="sm"
                  disabled={!continueRoute}
                  loading={continueQuery.isPending}
                  onClick={() =>
                    continueRoute && navigate(`${continueRoute}?context=READLIST&contextId=${readlist.id}`)
                  }
                >
                  <BookOpen className="size-4" /> {t('continue')}
                </Button>
                <Tooltip content={t('peekTooltip')}>
                  <Button
                    variant="secondary"
                    size="sm"
                    disabled={!continueRoute}
                    onClick={() =>
                      continueRoute && navigate(`${continueRoute}?context=READLIST&contextId=${readlist.id}&incognito=true`)
                    }
                  >
                    <EyeSlash className="size-4" /> {t('peek')}
                  </Button>
                </Tooltip>
              </>
            )}
            {admin && !editing && (
              <>
                <Button variant="secondary" size="sm" onClick={() => setAddBooksOpen(true)}>
                  <Plus className="size-4" /> {t('addBooks')}
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
                  <MenuItem onSelect={() => setEditOpen(true)}>
                    <PencilSimple className="size-4" /> {t('menu.editDetails')}
                  </MenuItem>
                  <MenuSeparator />
                  <MenuItem danger onSelect={() => setDeleteOpen(true)}>
                    <Trash className="size-4" /> {t('common:action.delete')}
                  </MenuItem>
                </Menu>
              </>
            )}
          </>
        }
      />

      {readlist.summary && <p className="-mt-3 mb-6 max-w-3xl text-sm text-ink-2">{readlist.summary}</p>}

      {editing ? (
        <EditReadListBooks readlist={readlist} onExit={() => setEditing(false)} />
      ) : (
        <>
          <FilterBar
            count={booksTotal}
            noun="books"
            groups={BOOK_DETAIL_FILTER_GROUPS}
            state={filters.state}
            activeCount={activeFilterCount(filters.state)}
            onToggleValue={filters.toggleValue}
            onToggleAuthor={filters.toggleAuthor}
            onQChange={filters.setQ}
            onOpenFilters={() => setDrawerOpen(true)}
            sortOptions={READLIST_BOOK_SORT_OPTIONS}
            sort={sort.current}
            onSortChange={sort.set}
          />

          {booksQuery.isPending ? (
            <GridSkeleton />
          ) : booksQuery.isLoadingError ? (
            <EmptyState
              title={t('empty.loadBooksFailed')}
              body={booksQuery.error.message}
              action={
                <Button variant="secondary" onClick={() => booksQuery.refetch()}>
                  {t('common:action.retry')}
                </Button>
              }
            />
          ) : books.length === 0 ? (
            <EmptyState
              title={t('empty.noBooks')}
              body={hasFilters ? t('empty.noBooksFiltered') : t('empty.readListEmpty')}
            />
          ) : (
            <>
              <MediaGrid>
                {books.map((b, i) => (
                  <div key={b.id} className="relative">
                    <BookCard book={b} showSeries />
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
        groups={BOOK_DETAIL_FILTER_GROUPS}
        state={filters.state}
        scope={{ readListId }}
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

      <EditReadListDialog open={editOpen} onOpenChange={setEditOpen} readlist={readlist} />
      <ConfirmDeleteDialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        title={t('readList.deleteTitle')}
        name={readlist.name}
        loading={deleteMutation.isPending}
        onConfirm={() => deleteMutation.mutate()}
      />
      <BookPickerDialog
        open={addBooksOpen}
        onOpenChange={setAddBooksOpen}
        title={t('addBooks')}
        confirmLabel={t('addToReadList')}
        mode="multi"
        excludeIds={new Set(readlist.bookIds)}
        onConfirm={(selected) => addBooksMutation.mutate(selected.map((b) => b.id))}
        confirming={addBooksMutation.isPending}
        error={addBooksMutation.error?.message}
      />
      <PosterManager
        open={postersOpen}
        onClose={() => setPostersOpen(false)}
        kind="readlist"
        entityId={readlist.id}
        title={readlist.name}
      />
    </div>
  )
}
