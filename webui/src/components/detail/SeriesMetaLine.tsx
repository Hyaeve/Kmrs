import type { ReactNode } from 'react'
import { Link } from 'react-router-dom'
import { FadeKey } from '@/components/ui/FadeKey'
import {
  ArrowDown,
  ArrowLeft,
  ArrowRight,
  CheckCircle,
  Globe,
  Lightning,
  PauseCircle,
  Scroll,
  WarningCircle,
} from '@phosphor-icons/react'
import type { SeriesMetadataDto } from '@/lib/api/types'
import { readingDirectionLabel, seriesStatusLabel, languageDisplayName } from '@/lib/utils/format'
import { cn } from '@/lib/utils/cn'

const iconCls = 'size-3.5 text-ink-3'

function seriesStatusIcon(status: string): ReactNode {
  switch (status) {
    case 'ONGOING':
      return <Lightning className={iconCls} />
    case 'ENDED':
      return <CheckCircle className={iconCls} />
    case 'ABANDONED':
      return <WarningCircle className={iconCls} />
    case 'HIATUS':
      return <PauseCircle className={iconCls} />
    default:
      return undefined
  }
}

function readingDirectionIcon(direction: string): ReactNode {
  switch (direction) {
    case 'LEFT_TO_RIGHT':
      return <ArrowRight className={iconCls} />
    case 'RIGHT_TO_LEFT':
      return <ArrowLeft className={iconCls} />
    case 'VERTICAL':
      return <ArrowDown className={iconCls} />
    case 'WEBTOON':
      return <Scroll className={iconCls} />
    default:
      return undefined
  }
}

interface MetaItem {
  key: string
  label: string
  icon?: ReactNode
  to?: string
}

/** status / age rating / language / reading direction of a series, as a quiet icon+text line */
export function SeriesMetaLine({ md, className }: { md: SeriesMetadataDto; className?: string }) {
  const items: MetaItem[] = []
  const status = seriesStatusLabel(md.status)
  if (status) items.push({ key: 'status', label: status, icon: seriesStatusIcon(md.status), to: `/series?seriesStatus=${md.status}` })
  if (md.ageRating != null) items.push({ key: 'age', label: `${md.ageRating}+`, to: `/series?ageRatings=${md.ageRating}` })
  if (md.language)
    items.push({ key: 'language', label: languageDisplayName(md.language), icon: <Globe className={iconCls} />, to: `/series?languages=${encodeURIComponent(md.language)}` })
  const direction = readingDirectionLabel(md.readingDirection)
  if (direction) items.push({ key: 'direction', label: direction, icon: readingDirectionIcon(md.readingDirection) })
  if (items.length === 0) return null
  return (
    <FadeKey
      id={items.map((i) => `${i.key}:${i.label}`).join('|')}
      className={cn('flex flex-wrap items-center gap-x-4 gap-y-1 text-sm text-ink-2', className)}
    >
      {items.map((item) =>
        item.to ? (
          <Link
            key={item.key}
            to={item.to}
            className="inline-flex items-center gap-1.5 transition-colors hover:text-accent-strong"
          >
            {item.icon}
            {item.label}
          </Link>
        ) : (
          <span key={item.key} className="inline-flex items-center gap-1.5">
            {item.icon}
            {item.label}
          </span>
        ),
      )}
    </FadeKey>
  )
}
