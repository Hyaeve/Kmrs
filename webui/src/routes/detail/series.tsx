import { useCallback, useMemo, useState } from 'react'
import { Link, Navigate, useNavigate, useParams } from 'react-router-dom'
import { keepPreviousData, useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Trans, useTranslation } from 'react-i18next'
import {
  ArrowCounterClockwise,
  ArrowSquareOut,
  ArrowsClockwise,
  BookOpen,
  Checks,
  Circle,
  CircleNotch,
  DotsThreeVertical,
  EyeSlash,
  FileMagnifyingGlass,
  FolderPlus,
  ImageSquare,
  PencilSimple,
  PlugsConnected,
  Sparkle,
  Trash,
} from '@phosphor-icons/react'
import { seriesApi } from '@/lib/api/series'
import { booksApi } from '@/lib/api/books'
import { librariesApi } from '@/lib/api/libraries'
import { komfApi } from '@/lib/api/komf'
import { canDownload, isAdmin, useAuthStore } from '@/lib/store/auth'
import { useDocumentTitle } from '@/lib/hooks/useDocumentTitle'
import { useKomfIntegration } from '@/lib/hooks/useKomfIntegration'
import { useBust } from '@/lib/store/thumbnails'
import { urls } from '@/lib/utils/urls'
import { readRoute } from '@/lib/utils/nav'
import { showToast } from '@/lib/store/toast'
import { trackKomfJob } from '@/lib/store/komfJobs'
import { Button } from '@/components/ui/Button'
import { IconButton } from '@/components/ui/IconButton'
import { Tooltip } from '@/components/ui/Tooltip'
import { BackButton } from '@/components/ui/BackButton'
import { Menu, MenuItem, MenuSeparator } from '@/components/ui/Menu'
import { EmptyState } from '@/components/ui/EmptyState'
import { GridSkeleton } from '@/components/ui/Skeleton'
import { CoverImage } from '@/components/media/CoverImage'
import { MediaGrid } from '@/components/media/MediaGrid'
import { useDensityCardWidth } from '@/lib/store/ui'
import { BookCard } from '@/components/media/BookCard'
import { CollectionCard } from '@/components/media/SeriesCard'
import { HorizontalRow } from '@/components/media/HorizontalRow'
import { DetailHero } from '@/components/detail/DetailHero'
import { DetailTitle } from '@/components/detail/DetailTitle'
import { DetailSkeleton } from '@/components/detail/DetailSkeleton'
import { DetailError } from '@/components/detail/DetailError'
import { DetailChipFlow } from '@/components/detail/DetailChipFlow'
import { SeriesMetaLine } from '@/components/detail/SeriesMetaLine'
import { creatorChipItems, genreChipItems, seriesTagChipItems, sharingLabelChipItems } from '@/components/detail/metadataChips'
import { Summary } from '@/components/detail/Summary'
import { DownloadLink } from '@/components/detail/DownloadLink'
import { AddToCollectionDialog } from '@/components/browse/AddToCollectionDialog'
import { ConfirmDeleteDialog } from '@/components/detail/ConfirmDeleteDialog'
import { RemoveFromCollectionButton } from '@/components/detail/MembershipRemoveButton'
import { EditSeriesDialog } from '@/components/metadata/EditSeriesDialog'
import { PosterManager } from '@/components/metadata/PosterManager'
import { KomfIdentifyDialog } from '@/components/metadata/KomfIdentifyDialog'
import { KomfResetDialog } from '@/components/metadata/KomfResetDialog'
import { TrackerSection } from '@/components/trackers/TrackerSection'
import { FilterBar } from '@/components/filters/FilterBar'
import { FilterDrawer } from '@/components/filters/FilterDrawer'
import { Sentinel } from '@/components/filters/Sentinel'
import { activeFilterCount, serializeFilters, useBrowseFilters } from '@/components/filters/filterUrl'
import { serializeSort, useSortState } from '@/components/filters/sort'
import { buildBookSearch } from '@/components/filters/builders'
import { BOOK_DETAIL_FILTER_GROUPS, SERIES_BOOK_DEFAULT_SORT, SERIES_BOOK_SORT_OPTIONS } from '@/components/filters/types'

const PAGE_SIZE = 48

// module-level so the hook's memo/effect deps stay stable across renders
const DISABLED_FILTERS = ['releaseYears'] as const

export function SeriesDetailPage() {
  const { t, i18n } = useTranslation('detail')
  const { seriesId = '' } = useParams()
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const user = useAuthStore((s) => s.user)
  const bust = useBust(seriesId)
  const rowCardWidth = useDensityCardWidth()
  const [addToCollectionOpen, setAddToCollectionOpen] = useState(false)
  const [editOpen, setEditOpen] = useState(false)
  const [postersOpen, setPostersOpen] = useState(false)
  const [identifyOpen, setIdentifyOpen] = useState(false)
  const [komfResetOpen, setKomfResetOpen] = useState(false)
  const [deleteOpen, setDeleteOpen] = useState(false)
  const komfReady = useKomfIntegration()

  const filters = useBrowseFilters('books:series', DISABLED_FILTERS)
  const sort = useSortState('books:series', SERIES_BOOK_DEFAULT_SORT)
  const [drawerOpen, setDrawerOpen] = useState(false)
  const hasFilters = activeFilterCount(filters.state) > 0 || !!filters.state.q.trim()

  const seriesQuery = useQuery({ queryKey: ['series', seriesId], queryFn: () => seriesApi.get(seriesId) })
  const series = seriesQuery.data
  const title = series ? series.metadata.title || series.name : ''

  const librariesQuery = useQuery({ queryKey: ['libraries'], queryFn: librariesApi.list })
  const collectionsQuery = useQuery({
    queryKey: ['collections', 'series', seriesId],
    queryFn: () => seriesApi.collections(seriesId),
    enabled: !!series && !series.oneshot,
  })
  // First unread (or in-progress) book, falling back to the first book when fully read
  const readTargetQuery = useQuery({
    queryKey: ['series', seriesId, 'read-target'],
    queryFn: async () => {
      const unread = await seriesApi.books(seriesId, { readStatus: ['UNREAD', 'IN_PROGRESS'], size: 1 })
      if (unread.content[0]) return unread.content[0]
      const first = await seriesApi.books(seriesId, { size: 1 })
      return first.content[0] ?? null
    },
    enabled: !!series && !series.oneshot,
  })
  const search = useMemo(
    () => buildBookSearch(filters.state, undefined, { seriesId: { operator: 'is', value: seriesId } }),
    [filters.state, seriesId],
  )
  const sortParam = serializeSort(sort.current)
  const filterKey = serializeFilters(filters.state)

  const booksQuery = useInfiniteQuery({
    queryKey: ['series', seriesId, 'books', filterKey, sortParam],
    queryFn: ({ pageParam }) => booksApi.list({ search, page: pageParam, size: PAGE_SIZE, sort: [sortParam] }),
    initialPageParam: 0,
    getNextPageParam: (last) => (last.last ? undefined : last.number + 1),
    placeholderData: keepPreviousData,
    enabled: !!series && !series.oneshot,
  })

  const markMutation = useMutation({
    mutationFn: (read: boolean) => (read ? seriesApi.markRead(seriesId) : seriesApi.markUnread(seriesId)),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['series'] })
      queryClient.invalidateQueries({ queryKey: ['books'] })
      queryClient.invalidateQueries({ queryKey: ['dashboard'] })
      queryClient.invalidateQueries({ queryKey: ['readlists'] })
    },
  })

  const analyzeMutation = useMutation({
    mutationFn: () => seriesApi.analyze(seriesId),
    onSuccess: () => showToast(t('toast.analysisQueued')),
    onError: (e) => showToast(e instanceof Error ? e.message : t('toast.queueAnalysisFailed')),
  })
  const refreshMutation = useMutation({
    mutationFn: () => seriesApi.refreshMetadata(seriesId),
    onSuccess: () => showToast(t('toast.refreshQueued')),
    onError: (e) => showToast(e instanceof Error ? e.message : t('toast.queueRefreshFailed')),
  })
  // komf PATCHes the metadata asynchronously once the job runs; the SSE SeriesChanged
  // invalidation picks the result up, so no query invalidation here
  const komfMatchMutation = useMutation({
    mutationFn: (libraryId: string) => komfApi.matchSeries(libraryId, seriesId),
    onSuccess: ({ id }) => trackKomfJob(id, title),
    onError: (e) => showToast(e instanceof Error ? e.message : t('toast.queueMatchFailed')),
  })
  const komfResetMutation = useMutation({
    mutationFn: (libraryId: string) => komfApi.resetSeries(libraryId, seriesId),
    onSuccess: () => {
      setKomfResetOpen(false)
      queryClient.invalidateQueries({ queryKey: ['series'] })
      queryClient.invalidateQueries({ queryKey: ['books'] })
      showToast(t('toast.metadataReset'))
    },
    onError: (e) => showToast(e instanceof Error ? e.message : t('toast.resetMetadataFailed')),
  })
  const deleteMutation = useMutation({
    mutationFn: () => seriesApi.deleteFile(seriesId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['series'] })
      queryClient.invalidateQueries({ queryKey: ['books'] })
      queryClient.invalidateQueries({ queryKey: ['dashboard'] })
      navigate('/series')
    },
    onError: (e) => showToast(e instanceof Error ? e.message : t('toast.deleteFilesFailed')),
  })

  useDocumentTitle(title || undefined)

  const { hasNextPage, isFetchingNextPage, isPlaceholderData, fetchNextPage } = booksQuery
  const loadMore = useCallback(() => {
    // isPlaceholderData means a stale query is shown while filters changed; don't page the old one
    if (hasNextPage && !isFetchingNextPage && !isPlaceholderData) void fetchNextPage()
  }, [hasNextPage, isFetchingNextPage, isPlaceholderData, fetchNextPage])

  if (seriesQuery.isPending) return <DetailSkeleton />
  if (seriesQuery.isLoadingError)
    return <DetailError error={seriesQuery.error} notFoundTitle={t('notFound.series')} onRetry={() => seriesQuery.refetch()} />
  if (!series) return null
  if (series.oneshot) return <Navigate to={`/oneshot/${series.id}`} replace />

  const md = series.metadata
  const library = librariesQuery.data?.find((l) => l.id === series.libraryId)
  // an unavailable library makes the series act deleted (komga parity)
  const unavailable = series.deleted || (library?.unavailable ?? false)
  const canMarkRead = series.booksUnreadCount > 0
  const canMarkUnread = series.booksReadCount + series.booksInProgressCount > 0
  const readTarget = readTargetQuery.data
  const readTargetRoute = readTarget ? readRoute(readTarget) : null
  const books = booksQuery.data?.pages.flatMap((p) => p.content) ?? []
  const booksTotal = booksQuery.data?.pages[0]?.totalElements
  const collections = collectionsQuery.data ?? []
  const cover = urls.seriesThumbnail(series.id, bust || undefined)

  const creatorItems = creatorChipItems(md.publisher, series.booksMetadata.authors, '/series')
  const genreItems = genreChipItems(md.genres)
  const tagItems = seriesTagChipItems(md.tags, series.booksMetadata.tags, i18n.language)
  const sharingItems = sharingLabelChipItems(md.sharingLabels)
  const hasMetadataFlows = genreItems.length + tagItems.length + sharingItems.length > 0

  return (
    <div>
      <DetailHero backdrop={cover} leading={<BackButton to="/series" className="mb-2 -ml-2" />}>
        <CoverImage src={cover} alt={title} eager className="w-36 shrink-0 shadow-card md:w-44" />
        <div className="min-w-0 flex-1">
          {library && (
            <p className="text-xs tracking-wide text-ink-3 uppercase">
              <Link to={`/libraries/${library.id}/series`} className="transition-colors hover:text-accent-strong">
                {library.name}
              </Link>
            </p>
          )}
          <DetailTitle title={title} />
          <DetailChipFlow items={creatorItems} className="mt-2.5" />
          <div className="mt-4 flex flex-wrap items-center gap-2">
            <Button
              variant="primary"
              disabled={!readTargetRoute}
              loading={readTargetQuery.isPending}
              onClick={() => readTargetRoute && navigate(readTargetRoute)}
            >
              <BookOpen className="size-4" />
              {t('read')}
            </Button>
            <Tooltip content={t('peekTooltip')}>
              <Button
                variant="secondary"
                disabled={!readTargetRoute}
                onClick={() => readTargetRoute && navigate(`${readTargetRoute}?incognito=true`)}
              >
                <EyeSlash className="size-4" />
                {t('peek')}
              </Button>
            </Tooltip>
            {canMarkRead && (
              <Button variant="secondary" loading={markMutation.isPending} onClick={() => markMutation.mutate(true)}>
                <Checks className="size-4" />
                {t('markAsRead')}
              </Button>
            )}
            {canMarkUnread && (
              <Button variant="secondary" loading={markMutation.isPending} onClick={() => markMutation.mutate(false)}>
                <Circle className="size-4" />
                {t('markAsUnread')}
              </Button>
            )}
            {canDownload(user) && <DownloadLink href={urls.seriesFile(series.id)} disabled={unavailable} />}
            {isAdmin(user) && (
              <Menu
                trigger={
                  <IconButton label={t('moreActions')}>
                    <DotsThreeVertical className="size-5" />
                  </IconButton>
                }
              >
                <MenuItem onSelect={() => setEditOpen(true)}>
                  <PencilSimple className="size-4" /> {t('menu.editMetadata')}
                </MenuItem>
                <MenuItem onSelect={() => setPostersOpen(true)}>
                  <ImageSquare className="size-4" /> {t('menu.managePosters')}
                </MenuItem>
                <MenuSeparator />
                <MenuItem onSelect={() => analyzeMutation.mutate()} disabled={analyzeMutation.isPending}>
                  <FileMagnifyingGlass className="size-4" /> {t('menu.analyze')}
                </MenuItem>
                <MenuItem onSelect={() => refreshMutation.mutate()} disabled={refreshMutation.isPending}>
                  <ArrowsClockwise className="size-4" /> {t('menu.refreshMetadata')}
                </MenuItem>
                {komfReady && (
                  <>
                    <MenuSeparator />
                    <MenuItem onSelect={() => setIdentifyOpen(true)}>
                      <Sparkle className="size-4" /> {t('menu.identifyKomf')}
                    </MenuItem>
                    <MenuItem
                      onSelect={() => komfMatchMutation.mutate(series.libraryId)}
                      disabled={komfMatchMutation.isPending}
                    >
                      <PlugsConnected className="size-4" /> {t('menu.matchKomf')}
                    </MenuItem>
                    <MenuItem onSelect={() => setKomfResetOpen(true)}>
                      <ArrowCounterClockwise className="size-4" /> {t('menu.resetKomf')}
                    </MenuItem>
                  </>
                )}
                <MenuSeparator />
                <MenuItem onSelect={() => setAddToCollectionOpen(true)}>
                  <FolderPlus className="size-4" /> {t('addToCollection')}
                </MenuItem>
                <MenuSeparator />
                <MenuItem danger onSelect={() => setDeleteOpen(true)}>
                  <Trash className="size-4" /> {t('menu.deleteFile')}
                </MenuItem>
              </Menu>
            )}
          </div>
          {unavailable && <p className="mt-2 text-xs font-medium text-danger">{t('common:state.unavailable')}</p>}
        </div>
      </DetailHero>

      <SeriesMetaLine md={md} className="mt-4" />

      {md.alternateTitles.length > 0 && (
        <div className="mt-3 space-y-0.5">
          {md.alternateTitles.map((t, i) => (
            <p key={`${t.label}-${i}`} className="text-xs text-ink-3">
              {t.title} ({t.label})
            </p>
          ))}
        </div>
      )}

      {md.summary && <Summary text={md.summary} className="mt-4" />}

      {hasMetadataFlows && (
        <div className="mt-4 space-y-2">
          <DetailChipFlow items={genreItems} />
          <DetailChipFlow items={tagItems} />
          <DetailChipFlow items={sharingItems} />
        </div>
      )}

      {md.links.length > 0 && (
        <div className="mt-3 flex flex-wrap gap-x-4 gap-y-1">
          {md.links.map((l) => (
            <a
              key={l.url}
              href={l.url}
              target="_blank"
              rel="noreferrer"
              className="inline-flex items-center gap-1 text-xs text-ink-3 transition-colors hover:text-accent-strong"
            >
              {l.label}
              <ArrowSquareOut className="size-3" />
            </a>
          ))}
        </div>
      )}

      <p className="mt-4 text-sm text-ink-3">
        <Trans
          i18nKey="detail:stats.books"
          count={series.booksCount}
          components={{ num: <span className="font-mono text-ink-2" /> }}
        />
        {' · '}
        <Trans
          i18nKey="detail:stats.read"
          count={series.booksReadCount}
          components={{ num: <span className="font-mono text-ink-2" /> }}
        />
        {' · '}
        <Trans
          i18nKey="detail:stats.unread"
          count={series.booksUnreadCount}
          components={{ num: <span className="font-mono text-ink-2" /> }}
        />
      </p>

      {collections.length > 0 && (
        <HorizontalRow title={t('inCollections')} collapsible className="mt-10">
          {collections.map((c) => (
            <div key={c.id} className="shrink-0" style={{ width: rowCardWidth }}>
              <CollectionCard
                id={c.id}
                name={c.name}
                count={c.seriesIds.length}
                actions={isAdmin(user) ? <RemoveFromCollectionButton collection={c} seriesId={series.id} /> : undefined}
              />
            </div>
          ))}
        </HorizontalRow>
      )}

      <TrackerSection seriesId={series.id} seriesTitle={title} libraryId={series.libraryId} metadataLinks={md.links} />

      <section className="mt-10">
        <h2 className="mb-4 font-display text-xl font-semibold text-ink">{t('booksHeading')}</h2>
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
          sortOptions={SERIES_BOOK_SORT_OPTIONS}
          sort={sort.current}
          onSortChange={sort.set}
        />
        {booksQuery.isPending ? (
          <GridSkeleton count={6} />
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
            body={hasFilters ? t('empty.noBooksFiltered') : t('empty.seriesEmpty')}
          />
        ) : (
          <>
            <MediaGrid>
              {books.map((b) => (
                <BookCard key={b.id} book={b} />
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
      </section>

      <AddToCollectionDialog
        open={addToCollectionOpen}
        onOpenChange={setAddToCollectionOpen}
        seriesIds={[series.id]}
        onDone={(_ok, message) => showToast(message)}
      />
      <FilterDrawer
        open={drawerOpen}
        onClose={() => setDrawerOpen(false)}
        groups={BOOK_DETAIL_FILTER_GROUPS}
        state={filters.state}
        scope={{ seriesId }}
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
      <EditSeriesDialog open={editOpen} onClose={() => setEditOpen(false)} seriesIds={[series.id]} />
      {komfReady && (
        <>
          <KomfIdentifyDialog
            open={identifyOpen}
            onOpenChange={setIdentifyOpen}
            series={series}
            onIdentified={() => showToast(t('toast.identifyQueued'))}
          />
          <KomfResetDialog
            open={komfResetOpen}
            onOpenChange={setKomfResetOpen}
            name={title}
            loading={komfResetMutation.isPending}
            onConfirm={() => komfResetMutation.mutate(series.libraryId)}
          />
        </>
      )}
      <PosterManager open={postersOpen} onClose={() => setPostersOpen(false)} kind="series" entityId={series.id} title={title} />
      <ConfirmDeleteDialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        title={t('deleteDialog.seriesFilesTitle')}
        name={t('deleteDialog.seriesFilesName', { name: title })}
        loading={deleteMutation.isPending}
        onConfirm={() => deleteMutation.mutate()}
      />
    </div>
  )
}
