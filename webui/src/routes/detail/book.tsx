import { useState, type ReactNode } from 'react'
import { Link, Navigate, useNavigate, useParams } from 'react-router-dom'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import {
  ArrowLeft,
  ArrowRight,
  ArrowSquareOut,
  ArrowsClockwise,
  BookmarkSimple,
  BookOpen,
  Checks,
  Circle,
  DotsThreeVertical,
  EyeSlash,
  FileMagnifyingGlass,
  ImageSquare,
  PencilSimple,
  Trash,
} from '@phosphor-icons/react'
import { booksApi } from '@/lib/api/books'
import { librariesApi } from '@/lib/api/libraries'
import { canDownload, isAdmin, useAuthStore } from '@/lib/store/auth'
import { useDocumentTitle } from '@/lib/hooks/useDocumentTitle'
import { useBust } from '@/lib/store/thumbnails'
import { urls } from '@/lib/utils/urls'
import { readRoute } from '@/lib/utils/nav'
import { mediaIssue } from '@/lib/utils/mediaStatus'
import { formatBytes, formatDate, relativeTime, fileNameFromUrl, filePathFromUrl } from '@/lib/utils/format'
import { cn } from '@/lib/utils/cn'
import { Button } from '@/components/ui/Button'
import { IconButton } from '@/components/ui/IconButton'
import { Tooltip } from '@/components/ui/Tooltip'
import { BackButton } from '@/components/ui/BackButton'
import { Menu, MenuItem, MenuSeparator } from '@/components/ui/Menu'
import { CoverImage } from '@/components/media/CoverImage'
import { ReadListCard } from '@/components/media/SeriesCard'
import { HorizontalRow } from '@/components/media/HorizontalRow'
import { useDensityCardWidth } from '@/lib/store/ui'
import { DetailHero } from '@/components/detail/DetailHero'
import { DetailTitle } from '@/components/detail/DetailTitle'
import { DetailSkeleton } from '@/components/detail/DetailSkeleton'
import { DetailError } from '@/components/detail/DetailError'
import { DetailChipFlow } from '@/components/detail/DetailChipFlow'
import { creatorChipItems, tagChipItems } from '@/components/detail/metadataChips'
import { Summary } from '@/components/detail/Summary'
import { DownloadLink } from '@/components/detail/DownloadLink'
import { AddToReadListDialog } from '@/components/detail/AddToReadListDialog'
import { ConfirmDeleteDialog } from '@/components/detail/ConfirmDeleteDialog'
import { RemoveFromReadListButton } from '@/components/detail/MembershipRemoveButton'
import { EditBooksDialog } from '@/components/metadata/EditBooksDialog'
import { PosterManager } from '@/components/metadata/PosterManager'
import { showToast } from '@/lib/store/toast'

function Field({ term, mono, children }: { term: string; mono?: boolean; children: ReactNode }) {
  return (
    <div>
      <dt className="text-xs tracking-wide text-ink-3 uppercase">{term}</dt>
      <dd className={cn('mt-1 text-sm text-ink', mono && 'font-mono')}>{children}</dd>
    </div>
  )
}

export function BookDetailPage() {
  const { t, i18n } = useTranslation('detail')
  const { bookId = '' } = useParams()
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const user = useAuthStore((s) => s.user)
  const bust = useBust(bookId)
  const rowCardWidth = useDensityCardWidth()
  const [addToListOpen, setAddToListOpen] = useState(false)
  const [editOpen, setEditOpen] = useState(false)
  const [postersOpen, setPostersOpen] = useState(false)
  const [deleteOpen, setDeleteOpen] = useState(false)

  const bookQuery = useQuery({ queryKey: ['books', bookId], queryFn: () => booksApi.get(bookId) })
  const book = bookQuery.data

  const librariesQuery = useQuery({ queryKey: ['libraries'], queryFn: librariesApi.list })
  // siblings 404 when there is none; the query client does not retry 404s
  const previousQuery = useQuery({
    queryKey: ['books', bookId, 'previous'],
    queryFn: () => booksApi.previous(bookId),
    enabled: !!book && !book.oneshot,
  })
  const nextQuery = useQuery({
    queryKey: ['books', bookId, 'next'],
    queryFn: () => booksApi.next(bookId),
    enabled: !!book && !book.oneshot,
  })
  const readlistsQuery = useQuery({
    queryKey: ['readlists', 'book', bookId],
    queryFn: () => booksApi.readlists(bookId),
    enabled: !!book && !book.oneshot,
  })

  const markMutation = useMutation({
    mutationFn: (read: boolean) => (read ? booksApi.markRead(bookId) : booksApi.markUnread(bookId)),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['books'] })
      queryClient.invalidateQueries({ queryKey: ['series'] })
      queryClient.invalidateQueries({ queryKey: ['dashboard'] })
      queryClient.invalidateQueries({ queryKey: ['readlists'] })
    },
  })

  const analyzeMutation = useMutation({
    mutationFn: () => booksApi.analyze(bookId),
    onSuccess: () => showToast(t('toast.analysisQueued')),
    onError: (e) => showToast(e instanceof Error ? e.message : t('toast.queueAnalysisFailed')),
  })
  const refreshMutation = useMutation({
    mutationFn: () => booksApi.refreshMetadata(bookId),
    onSuccess: () => showToast(t('toast.refreshQueued')),
    onError: (e) => showToast(e instanceof Error ? e.message : t('toast.queueRefreshFailed')),
  })
  const deleteMutation = useMutation({
    mutationFn: () => booksApi.deleteFile(bookId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['books'] })
      queryClient.invalidateQueries({ queryKey: ['series'] })
      queryClient.invalidateQueries({ queryKey: ['dashboard'] })
      if (book?.oneshot) navigate('/series')
      else navigate(`/series/${book?.seriesId ?? ''}`)
    },
    onError: (e) => showToast(e instanceof Error ? e.message : t('toast.deleteFileFailed')),
  })

  const title = book ? book.metadata.title || book.name : ''
  useDocumentTitle(title || undefined)

  if (bookQuery.isPending) return <DetailSkeleton />
  if (bookQuery.isLoadingError)
    return <DetailError error={bookQuery.error} notFoundTitle={t('notFound.book')} onRetry={() => bookQuery.refetch()} />
  if (!book) return null
  if (book.oneshot) return <Navigate to={`/oneshot/${book.seriesId}`} replace />

  const library = librariesQuery.data?.find((l) => l.id === book.libraryId)
  const md = book.metadata
  const progress = book.readProgress
  const completed = !!progress?.completed
  // an unavailable library makes the book act deleted (komga parity)
  const unavailable = book.deleted || (library?.unavailable ?? false)
  const route = readRoute({ id: book.id, media: book.media, deleted: unavailable })
  const issue = mediaIssue({ media: book.media, deleted: unavailable })
  const prev = previousQuery.data ?? null
  const next = nextQuery.data ?? null
  const readlists = readlistsQuery.data ?? []
  const cover = urls.bookThumbnail(book.id, bust || undefined)
  const authorItems = creatorChipItems('', md.authors, '/books')
  const tagItems = tagChipItems(md.tags, '/books', i18n.language)
  const progressPct = completed ? 100 : book.media.pagesCount > 0 && progress ? (progress.page / book.media.pagesCount) * 100 : 0

  return (
    <div>
      <DetailHero backdrop={cover} leading={<BackButton to={`/series/${book.seriesId}`} className="mb-2 -ml-2" />}>
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
          <p className="mt-1.5 text-ink-2">
            <Link to={`/series/${book.seriesId}`} className="transition-colors hover:text-accent-strong">
              {book.seriesTitle}
            </Link>
            {' · '}
            <span className="font-mono">#{md.number}</span>
          </p>
          <DetailChipFlow items={authorItems} className="mt-2.5" />
          <div className="mt-4 flex flex-wrap items-center gap-2">
            <Button variant="primary" disabled={!route} onClick={() => route && navigate(route)}>
              <BookOpen className="size-4" />
              {progress && !progress.completed ? t('continueWithPage', { page: progress.page }) : t('read')}
            </Button>
            <Tooltip content={t('peekTooltip')}>
              <Button variant="secondary" disabled={!route} onClick={() => route && navigate(`${route}?incognito=true`)}>
                <EyeSlash className="size-4" />
                {t('peek')}
              </Button>
            </Tooltip>
            {!completed && (
              <Button variant="secondary" loading={markMutation.isPending} onClick={() => markMutation.mutate(true)}>
                <Checks className="size-4" />
                {t('markAsRead')}
              </Button>
            )}
            {progress && (
              <Button variant="secondary" loading={markMutation.isPending} onClick={() => markMutation.mutate(false)}>
                <Circle className="size-4" />
                {t('markAsUnread')}
              </Button>
            )}
            {canDownload(user) && <DownloadLink href={urls.bookFile(book.id)} disabled={unavailable} />}
            {(isAdmin(user) || prev || next) && (
              <Menu
                trigger={
                  <IconButton label={t('moreActions')}>
                    <DotsThreeVertical className="size-5" />
                  </IconButton>
                }
              >
                {isAdmin(user) && (
                  <>
                    <MenuItem onSelect={() => setEditOpen(true)}>
                      <PencilSimple className="size-4" /> {t('menu.editMetadata')}
                    </MenuItem>
                    <MenuItem onSelect={() => setPostersOpen(true)}>
                      <ImageSquare className="size-4" /> {t('menu.managePosters')}
                    </MenuItem>
                    <MenuItem onSelect={() => analyzeMutation.mutate()} disabled={analyzeMutation.isPending}>
                      <FileMagnifyingGlass className="size-4" /> {t('menu.analyze')}
                    </MenuItem>
                    <MenuItem onSelect={() => refreshMutation.mutate()} disabled={refreshMutation.isPending}>
                      <ArrowsClockwise className="size-4" /> {t('menu.refreshMetadata')}
                    </MenuItem>
                    <MenuItem onSelect={() => setAddToListOpen(true)}>
                      <BookmarkSimple className="size-4" /> {t('addToReadList')}
                    </MenuItem>
                  </>
                )}
                {isAdmin(user) && (prev || next) && <MenuSeparator />}
                {prev && (
                  <MenuItem onSelect={() => navigate(`/book/${prev.id}`)}>
                    <ArrowLeft className="size-4" /> {t('menu.previousBook')}
                  </MenuItem>
                )}
                {next && (
                  <MenuItem onSelect={() => navigate(`/book/${next.id}`)}>
                    <ArrowRight className="size-4" /> {t('menu.nextBook')}
                  </MenuItem>
                )}
                {isAdmin(user) && <MenuSeparator />}
                {isAdmin(user) && (
                  <MenuItem danger onSelect={() => setDeleteOpen(true)}>
                    <Trash className="size-4" /> {t('menu.deleteFile')}
                  </MenuItem>
                )}
              </Menu>
            )}
          </div>
          {issue && (
            <p
              className={cn(
                'mt-2 text-xs',
                issue.severity === 'danger'
                  ? 'text-danger'
                  : issue.severity === 'accent'
                    ? 'text-accent-strong'
                    : 'text-ink-3',
              )}
            >
              {issue.title}
              {issue.detail && `: ${issue.detail}`}
            </p>
          )}
        </div>
      </DetailHero>

      {md.summary && <Summary text={md.summary} className="mt-6" />}

      <DetailChipFlow items={tagItems} className="mt-4" />

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

      <section className="mt-8 rounded-xl border border-line bg-surface p-5">
        <dl className="grid grid-cols-1 gap-x-8 gap-y-4 sm:grid-cols-2">
          <Field term={t('field.pages')} mono>
            {book.media.pagesCount}
          </Field>
          <Field term={t('field.size')} mono>
            {formatBytes(book.sizeBytes)}
          </Field>
          <Field term={t('field.format')}>{book.media.mediaType}</Field>
          <Field term={t('field.profile')}>{book.media.mediaProfile}</Field>
          {md.releaseDate && <Field term={t('field.releaseDate')}>{formatDate(md.releaseDate)}</Field>}
          {md.isbn && (
            <Field term="ISBN" mono>
              {md.isbn}
            </Field>
          )}
          <Field term={t('field.file')} mono>
            <Tooltip content={filePathFromUrl(book.url)}>
              <span className="block truncate">{fileNameFromUrl(book.url)}</span>
            </Tooltip>
          </Field>
          <Field term={t('field.added')}>{formatDate(book.created)}</Field>
        </dl>

        {progress && (
          <div className="mt-5 border-t border-line pt-4">
            <p className="text-sm text-ink-2">
              {completed ? t('progress.completed') : t('progress.pageOf', { page: progress.page, total: book.media.pagesCount })}
              {progress.readDate && <span className="text-ink-3"> · {relativeTime(progress.readDate)}</span>}
            </p>
            <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-overlay">
              <div className="h-full rounded-full bg-accent transition-[width] duration-300" style={{ width: `${progressPct}%` }} />
            </div>
          </div>
        )}
      </section>

      {readlists.length > 0 && (
        <HorizontalRow title={t('inReadLists')} className="mt-10">
          {readlists.map((l) => (
            <div key={l.id} className="shrink-0" style={{ width: rowCardWidth }}>
              <ReadListCard
                id={l.id}
                name={l.name}
                count={l.bookIds.length}
                actions={isAdmin(user) ? <RemoveFromReadListButton readlist={l} bookId={book.id} /> : undefined}
              />
            </div>
          ))}
        </HorizontalRow>
      )}

      {(prev || next) && (
        <nav className="mt-10 grid grid-cols-2 gap-4 border-t border-line pt-6">
          <div className="min-w-0">
            {prev && (
              <Link to={`/book/${prev.id}`} className="group inline-block max-w-full">
                <span className="flex items-center gap-1 text-xs tracking-wide text-ink-3 uppercase">
                  <ArrowLeft className="size-3.5" /> {t('previous')}
                </span>
                <span className="mt-1 block truncate text-sm text-ink transition-colors group-hover:text-accent-strong">
                  #{prev.metadata.number} {prev.metadata.title || prev.name}
                </span>
              </Link>
            )}
          </div>
          <div className="min-w-0 text-right">
            {next && (
              <Link to={`/book/${next.id}`} className="group inline-block max-w-full">
                <span className="flex items-center justify-end gap-1 text-xs tracking-wide text-ink-3 uppercase">
                  {t('next')} <ArrowRight className="size-3.5" />
                </span>
                <span className="mt-1 block truncate text-sm text-ink transition-colors group-hover:text-accent-strong">
                  #{next.metadata.number} {next.metadata.title || next.name}
                </span>
              </Link>
            )}
          </div>
        </nav>
      )}

      <AddToReadListDialog bookId={book.id} open={addToListOpen} onOpenChange={setAddToListOpen} />
      <EditBooksDialog open={editOpen} onClose={() => setEditOpen(false)} bookIds={[book.id]} />
      <PosterManager open={postersOpen} onClose={() => setPostersOpen(false)} kind="book" entityId={book.id} title={title} />
      <ConfirmDeleteDialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        title={t('deleteDialog.bookFileTitle')}
        name={t('deleteDialog.bookFileName', { name: title })}
        loading={deleteMutation.isPending}
        onConfirm={() => deleteMutation.mutate()}
      />
    </div>
  )
}
