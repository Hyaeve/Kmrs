import type { BookDto } from '@/lib/api/types'
import { useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { CheckCircle, EyeSlash, Play } from '@phosphor-icons/react'
import { urls } from '@/lib/utils/urls'
import { bookDetailRoute, readRoute } from '@/lib/utils/nav'
import { useBust } from '@/lib/store/thumbnails'
import { useUiStore } from '@/lib/store/ui'
import { CoverImage } from './CoverImage'
import { CardFrame, CardMenuButton, CardText, CardOverlayText } from './CardFrame'
import { ProgressCapsule, OneshotLine } from './badges'
import { BookCardMenu } from './BookCardMenu'
import { MenuItem } from '@/components/ui/Menu'
import { SelectBadge } from '@/components/selection/SelectBadge'
import type { CardSelection } from '@/components/selection/useSelection'
import { relativeTime } from '@/lib/utils/format'
import { cardStatusLabel } from '@/lib/utils/mediaStatus'
import { useLibraryUnavailable } from '@/lib/utils/libraries'
import { cn } from '@/lib/utils/cn'

interface BookCardProps {
  book: BookDto
  className?: string
  /** show the series title as secondary line (useful outside series context) */
  showSeries?: boolean
  eager?: boolean
  /** when set, the card shows a selection checkbox and toggles instead of navigating in selection mode */
  selection?: CardSelection
}

export function BookCard({ book, className, showSeries, eager, selection }: BookCardProps) {
  const { t } = useTranslation('media')
  const navigate = useNavigate()
  const bust = useBust(book.id)
  const blurUnread = useUiStore((s) => s.blurUnreadCovers)
  const title = book.metadata.title || book.name
  const unread = !book.readProgress

  const progress = book.readProgress && !book.readProgress.completed && book.media.pagesCount > 0
    ? book.readProgress.page / book.media.pagesCount
    : 0

  // meta line: in-progress "#3 · 45% · 120 pages", completed "#3 · ✓ 3d ago"; oneshots omit the number (single book)
  const readAgo = book.readProgress?.completed ? relativeTime(book.readProgress.readDate) : ''
  const metaParts: string[] = []
  if (!book.oneshot) metaParts.push(`#${book.metadata.number}`)
  if (progress > 0) metaParts.push(`${Math.round(progress * 100)}%`)
  metaParts.push(t('card.pageCount', { count: book.media.pagesCount }))
  const secondary = readAgo ? (
    <span className="inline-flex items-center gap-1">
      {!book.oneshot && `#${book.metadata.number} · `}
      <CheckCircle className="size-3" weight="fill" />
      {readAgo}
    </span>
  ) : (
    metaParts.join(' · ')
  )

  // oneshots show the OneshotLine in the series-title slot; the title still clamps to one line
  const overline = book.oneshot ? (
    <span className="inline-flex items-center gap-1">
      <OneshotLine authors={book.metadata.authors} />
    </span>
  ) : showSeries ? (
    book.seriesTitle
  ) : undefined
  const titleLines = showSeries || book.oneshot ? 1 : 2

  // like komga's card body line: unavailable or broken media replaces the normal meta
  const libraryUnavailable = useLibraryUnavailable(book.libraryId)
  const readTo = readRoute({ id: book.id, media: book.media, deleted: book.deleted || libraryUnavailable })
  const statusLabel = cardStatusLabel({ deleted: book.deleted || libraryUnavailable, mediaStatus: book.media.status })
  const secondaryText = statusLabel ? (
    <span className={statusLabel.className}>{statusLabel.text}</span>
  ) : (
    secondary
  )

  const frame = (
    <CardFrame
      to={bookDetailRoute(book)}
      label={title}
      className={selection ? undefined : className}
      actions={
        selection?.active ? undefined : (
          <BookCardMenu
            book={book}
            navItem={
              <>
                <MenuItem onSelect={() => navigate(readTo ?? bookDetailRoute(book))}>
                  <Play className="size-4" /> {t('menu.read')}
                </MenuItem>
                {readTo && (
                  <MenuItem onSelect={() => navigate(`${readTo}?incognito=true`)}>
                    <EyeSlash className="size-4" /> {t('menu.peek')}
                  </MenuItem>
                )}
              </>
            }
            onSelect={selection ? () => selection.onToggle() : undefined}
            trigger={<CardMenuButton aria-label={t('card.actionsFor', { title })} />}
          />
        )
      }
    >
      <div className="relative">
        <CoverImage
          src={urls.bookThumbnail(book.id, bust || undefined)}
          alt={title}
          eager={eager}
          blurred={blurUnread && unread}
          className={cn(selection?.selected && 'ring-2 ring-accent')}
        />
        {selection?.active && <SelectBadge {...selection} label={title} />}
        <CardOverlayText title={title} overline={overline} secondary={secondaryText} titleLines={titleLines} />
      </div>
      <ProgressCapsule value={progress} />
      <CardText title={title} overline={overline} secondary={secondaryText} titleLines={titleLines} />
    </CardFrame>
  )

  if (!selection) return frame
  // in selection mode the whole card toggles; capture phase so both the badge and the Link see one click
  return (
    <div
      // select-none: shift-click range selection must not grow a browser text selection
      className={cn('rounded-lg', selection.active && 'select-none', className)}
      onClickCapture={(e) => {
        if (!selection.active) return
        e.preventDefault()
        e.stopPropagation()
        selection.onToggle(e.shiftKey)
      }}
    >
      {frame}
    </div>
  )
}
