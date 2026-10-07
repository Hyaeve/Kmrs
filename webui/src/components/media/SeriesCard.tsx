import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import type { TFunction } from 'i18next'
import type { SeriesDto } from '@/lib/api/types'
import { CheckCircle } from '@phosphor-icons/react'
import { urls } from '@/lib/utils/urls'
import { useBust } from '@/lib/store/thumbnails'
import { useUiStore } from '@/lib/store/ui'
import { cardStatusLabel } from '@/lib/utils/mediaStatus'
import { useLibraryUnavailable } from '@/lib/utils/libraries'
import { CoverImage } from './CoverImage'
import { CardFrame, CardMenuButton, CardText, CardOverlayText } from './CardFrame'
import { ProgressCapsule, UnreadBadge, OneshotLine } from './badges'
import { SeriesCardMenu } from './SeriesCardMenu'
import { SelectBadge } from '@/components/selection/SelectBadge'
import type { CardSelection } from '@/components/selection/useSelection'
import { cn } from '@/lib/utils/cn'

function secondaryLine(series: SeriesDto, t: TFunction): ReactNode {
  const { booksCount, booksUnreadCount, booksInProgressCount, booksReadCount } = series
  // oneshot series show the OneshotLine (checkmark when read) instead of a meaningless "1 books"
  if (series.oneshot) {
    return (
      <span className="inline-flex items-center gap-1">
        {booksReadCount > 0 && <CheckCircle className="size-3" weight="fill" />}
        <OneshotLine authors={series.booksMetadata.authors} />
      </span>
    )
  }
  if (booksInProgressCount > 0 && booksCount > 0) {
    return t('series.progressSummary', { pct: Math.round((booksReadCount / booksCount) * 100), count: booksCount })
  }
  if (booksUnreadCount > 0) return t('series.unreadSummary', { unread: booksUnreadCount, total: booksCount })
  return t('series.booksCount', { count: booksCount })
}

interface SeriesCardProps {
  series: SeriesDto
  className?: string
  eager?: boolean
  /** when set, the card shows a selection checkbox and toggles instead of navigating in selection mode */
  selection?: CardSelection
}

export function SeriesCard({ series, className, eager, selection }: SeriesCardProps) {
  const { t } = useTranslation('media')
  const bust = useBust(series.id)
  const blurUnread = useUiStore((s) => s.blurUnreadCovers)
  const title = series.metadata.title || series.name

  // like komga: an unavailable series replaces its status line
  const libraryUnavailable = useLibraryUnavailable(series.libraryId)
  const statusLabel = cardStatusLabel({ deleted: series.deleted || libraryUnavailable })
  const secondary = statusLabel ? <span className={statusLabel.className}>{statusLabel.text}</span> : secondaryLine(series, t)

  const frame = (
    <CardFrame
      to={series.oneshot ? `/oneshot/${series.id}` : `/series/${series.id}`}
      label={title}
      className={selection ? undefined : className}
      actions={
        selection?.active ? undefined : (
          <SeriesCardMenu
            series={series}
            onSelect={selection ? () => selection.onToggle() : undefined}
            trigger={<CardMenuButton aria-label={t('card.actionsFor', { title })} />}
          />
        )
      }
    >
      <div className="relative">
        <CoverImage
          src={urls.seriesThumbnail(series.id, bust || undefined)}
          alt={title}
          eager={eager}
          blurred={blurUnread && series.booksUnreadCount > 0}
          className={cn(selection?.selected && 'ring-2 ring-accent')}
        />
        {selection?.active && <SelectBadge {...selection} label={title} />}
        <UnreadBadge count={series.booksUnreadCount} show={!selection?.active} />
        <CardOverlayText title={title} secondary={secondary} />
      </div>
      <ProgressCapsule
        value={series.booksCount > 0 && series.booksInProgressCount > 0 ? series.booksReadCount / series.booksCount : 0}
      />
      <CardText title={title} secondary={secondary} />
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

export function CollectionCard({
  id,
  name,
  count,
  className,
  actions,
}: {
  id: string
  name: string
  count: number
  className?: string
  /** overlay controls rendered next to the link (see CardFrame) */
  actions?: ReactNode
}) {
  const { t } = useTranslation('media')
  const bust = useBust(id)
  const secondary = t('collection.seriesCount', { count })
  return (
    <CardFrame to={`/collections/${id}`} label={name} className={className} actions={actions}>
      <div className={cn('relative')}>
        <CoverImage src={urls.collectionThumbnail(id, bust || undefined)} alt={name} />
        <CardOverlayText title={name} secondary={secondary} />
      </div>
      <CardText title={name} secondary={secondary} />
    </CardFrame>
  )
}

export function ReadListCard({
  id,
  name,
  count,
  className,
  actions,
}: {
  id: string
  name: string
  count: number
  className?: string
  /** overlay controls rendered next to the link (see CardFrame) */
  actions?: ReactNode
}) {
  const { t } = useTranslation('media')
  const bust = useBust(id)
  const secondary = t('readList.bookCount', { count })
  return (
    <CardFrame to={`/readlists/${id}`} label={name} className={className} actions={actions}>
      <div className="relative">
        <CoverImage src={urls.readlistThumbnail(id, bust || undefined)} alt={name} />
        <CardOverlayText title={name} secondary={secondary} />
      </div>
      <CardText title={name} secondary={secondary} />
    </CardFrame>
  )
}
