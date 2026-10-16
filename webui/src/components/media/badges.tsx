import { useTranslation } from 'react-i18next'
import { Book } from '@phosphor-icons/react'
import type { AuthorDto } from '@/lib/api/types'
import { primaryAuthor } from '@/lib/utils/authors'
import { cn } from '@/lib/utils/cn'

/** Slim progress bar under the cover; the strip keeps its height even at 0 progress
    so card text stays aligned across a grid row. */
export function ProgressCapsule({ value, className }: { value: number; className?: string }) {
  const pct = Math.max(0, Math.min(1, value))
  return (
    <div aria-hidden className={cn('mt-1 h-[3px] px-1', className)}>
      {pct > 0 && (
        <div className="h-full rounded-full bg-line-strong">
          <div
            className="h-full rounded-full bg-accent transition-[width] duration-300"
            style={{ width: `${Math.max(pct * 100, 4)}%` }}
          />
        </div>
      )}
    </div>
  )
}

/** Floating unread-count capsule, overlapping the cover's top-right corner. `show` lets the
    card suppress it in selection mode, where it fights the SelectBadge for attention. */
export function UnreadBadge({ count, show = true, className }: { count: number; show?: boolean; className?: string }) {
  const visible = show && count > 0
  return (
    <span
      aria-hidden={!visible}
      className={cn(
        'pointer-events-none absolute -top-1.5 -right-1.5 flex h-5.5 min-w-5.5 origin-top-right items-center justify-center rounded-full px-1.5',
        'bg-accent text-[11px] font-semibold text-white tabular-nums',
        'shadow-[0_1px_4px_rgb(0_0_0/0.45),inset_0_1px_0_rgb(255_255_255/0.25)] ring-2 ring-bg',
        'transition-[opacity,transform] duration-200 ease-out-expo',
        visible ? 'scale-100 opacity-100' : 'scale-50 opacity-0',
        className,
      )}
    >
      {/* keyed so a count change replays the pop */}
      <span key={count} className="animate-badge-pop">
        {count > 99 ? '99+' : count}
      </span>
    </span>
  )
}

/** Oneshot marker for card text lines: a single-book icon (vs. the Books stack of
    series) followed by the primary author, so the line carries useful info instead
    of just the "Oneshot" label. Render as a fragment so the caller controls layout. */
export function OneshotLine({ authors }: { authors: AuthorDto[] }) {
  const { t } = useTranslation()
  return (
    <>
      <Book className="size-3 shrink-0" />
      {primaryAuthor(authors)?.name ?? t('oneshot')}
    </>
  )
}
