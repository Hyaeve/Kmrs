import { useState } from 'react'
import { Link } from 'react-router-dom'
import { keepPreviousData, useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { ArrowClockwise, MagnifyingGlass, Sparkle, WarningCircle } from '@phosphor-icons/react'
import { booksApi } from '@/lib/api/books'
import { librariesApi } from '@/lib/api/libraries'
import type { BookDto, BookSearch, MediaStatus, SearchCondition } from '@/lib/api/types'
import { urls } from '@/lib/utils/urls'
import { bookDetailRoute } from '@/lib/utils/nav'
import { convertErrorCodes } from '@/lib/utils/mediaStatus'
import { cn } from '@/lib/utils/cn'
import { useDocumentTitle } from '@/lib/hooks/useDocumentTitle'
import { Button } from '@/components/ui/Button'
import { EmptyState } from '@/components/ui/EmptyState'
import { PageHeader } from '@/components/ui/PageHeader'
import { Skeleton } from '@/components/ui/Skeleton'
import { LibraryFilterMenu } from '@/components/admin/LibraryFilterMenu'
import { Sentinel } from '@/components/filters/Sentinel'

const STATUSES: MediaStatus[] = ['ERROR', 'UNSUPPORTED']

function searchFor(libraryId: string | null): BookSearch {
  const conditions: SearchCondition[] = [
    // deleted books are not actionable, they only add noise
    { deleted: { operator: 'isFalse' } },
    { anyOf: STATUSES.map((s) => ({ mediaStatus: { operator: 'is' as const, value: s } })) },
  ]
  if (libraryId) conditions.unshift({ libraryId: { operator: 'is', value: libraryId } })
  return { condition: { allOf: conditions } }
}

function StatusBadge({ status }: { status: MediaStatus }) {
  const { t } = useTranslation('admin-maintenance')
  return (
    <span
      className={cn(
        'inline-flex shrink-0 items-center rounded-full border px-2 py-0.5 text-[11px] font-medium',
        status === 'ERROR' ? 'border-danger/40 bg-danger/10 text-danger' : 'border-accent/40 bg-accent-soft text-accent-strong',
      )}
    >
      {status === 'ERROR' ? t('common:cardStatus.error') : t('common:cardStatus.unsupported')}
    </span>
  )
}

function Row({ book, libraryName }: { book: BookDto; libraryName: string }) {
  const { t } = useTranslation('admin-maintenance')
  const queryClient = useQueryClient()
  const [feedback, setFeedback] = useState<string | null>(null)

  const invalidate = () => {
    void queryClient.invalidateQueries({ queryKey: ['admin', 'media-analysis'] })
    void queryClient.invalidateQueries({ queryKey: ['books'] })
  }
  const act = useMutation({
    mutationFn: (kind: 'analyze' | 'refresh') => (kind === 'analyze' ? booksApi.analyze(book.id) : booksApi.refreshMetadata(book.id)),
    onSuccess: (_data, kind) => {
      setFeedback(kind === 'analyze' ? t('detail:toast.analysisQueued') : t('mediaAnalysis.refreshQueued'))
      invalidate()
    },
  })

  return (
    <li className="flex items-center gap-3 px-4 py-3">
      <Link to={bookDetailRoute(book)} className="shrink-0">
        <div
          aria-hidden
          className="h-14 w-10 rounded-md bg-raised bg-cover bg-center"
          style={{ backgroundImage: `url(${urls.bookThumbnail(book.id)})` }}
        />
      </Link>
      <div className="min-w-0 flex-1">
        <p className="truncate text-sm text-ink">
          <Link to={bookDetailRoute(book)} className="hover:text-accent-strong">
            {book.seriesTitle}
          </Link>
          <span className="text-ink-3"> · {book.metadata.title || book.name}</span>
        </p>
        <p className="mt-0.5 truncate text-xs text-ink-3" title={book.media.comment || undefined}>
          {book.media.comment ? convertErrorCodes(book.media.comment) : t('mediaAnalysis.noDetails')}
        </p>
        {act.isError && (
          <p className="mt-0.5 text-xs text-danger">
            {act.error instanceof Error ? act.error.message : t('mediaAnalysis.actionFailed')}
          </p>
        )}
        {feedback && !act.isError && <p className="mt-0.5 text-xs text-accent-strong">{feedback}</p>}
      </div>
      <StatusBadge status={book.media.status} />
      <span className="hidden w-32 truncate text-xs text-ink-3 md:inline" title={libraryName}>
        {libraryName}
      </span>
      <div className="flex shrink-0 gap-1.5">
        <Button size="sm" onClick={() => act.mutate('analyze')} disabled={act.isPending}>
          <MagnifyingGlass className="size-4" />
          {t('detail:menu.analyze')}
        </Button>
        <Button size="sm" onClick={() => act.mutate('refresh')} disabled={act.isPending}>
          <ArrowClockwise className="size-4" />
          {t('detail:menu.refreshMetadata')}
        </Button>
      </div>
    </li>
  )
}

export function AdminMediaAnalysisPage() {
  const { t } = useTranslation('admin-maintenance')
  const [libraryId, setLibraryId] = useState<string | null>(null)
  const librariesQuery = useQuery({ queryKey: ['libraries'], queryFn: librariesApi.list })

  useDocumentTitle(t('layout:nav.mediaAnalysis'))

  const q = useInfiniteQuery({
    queryKey: ['admin', 'media-analysis', libraryId ?? 'all'],
    queryFn: ({ pageParam }) =>
      booksApi.list({ search: searchFor(libraryId), page: pageParam, size: 50, sort: ['series,asc'] }),
    getNextPageParam: (last) => (last.last ? undefined : last.number + 1),
    initialPageParam: 0,
    placeholderData: keepPreviousData,
  })

  const items = q.data?.pages.flatMap((p) => p.content) ?? []
  const total = q.data?.pages[0]?.totalElements
  const libraryName = (id: string) => librariesQuery.data?.find((l) => l.id === id)?.name ?? t('unknownLibrary')

  return (
    <div className="max-w-5xl">
      <PageHeader
        title={t('layout:nav.mediaAnalysis')}
        subtitle={total !== undefined ? t('mediaAnalysis.needingAttention', { count: total }) : t('mediaAnalysis.subtitleFallback')}
        actions={<LibraryFilterMenu value={libraryId} onChange={setLibraryId} />}
      />

      {q.isPending ? (
        <div className="flex flex-col gap-2.5">
          {Array.from({ length: 5 }, (_, i) => (
            <Skeleton key={i} className="h-20 w-full" />
          ))}
        </div>
      ) : q.isLoadingError ? (
        <EmptyState
          icon={<WarningCircle />}
          title={t('browse:books.loadError')}
          body={q.error instanceof Error ? q.error.message : t('errorFallback')}
          action={<Button onClick={() => q.refetch()}>{t('common:action.retry')}</Button>}
        />
      ) : items.length === 0 ? (
        <EmptyState
          icon={<Sparkle />}
          title={t('mediaAnalysis.emptyTitle')}
          body={t('mediaAnalysis.emptyBody')}
        />
      ) : (
        <>
          <ul className="flex flex-col divide-y divide-line rounded-xl border border-line bg-surface">
            {items.map((b) => (
              <Row key={b.id} book={b} libraryName={libraryName(b.libraryId)} />
            ))}
          </ul>
          <Sentinel
            active={!!q.hasNextPage && !q.isPlaceholderData}
            onIntersect={() => {
              if (q.hasNextPage && !q.isFetchingNextPage) void q.fetchNextPage()
            }}
          />
          {q.isFetchingNextPage && <Skeleton className="mt-3 h-16 w-full" />}
        </>
      )}
    </div>
  )
}
