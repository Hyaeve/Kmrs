import { useCallback, useEffect, useState } from 'react'
import { keepPreviousData, useInfiniteQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Check, MagnifyingGlass } from '@phosphor-icons/react'
import { booksApi } from '@/lib/api/books'
import type { BookDto } from '@/lib/api/types'
import { useChanged } from '@/lib/hooks/useChanged'
import { urls } from '@/lib/utils/urls'
import { cn } from '@/lib/utils/cn'
import { Dialog } from '@/components/ui/Dialog'
import { Button } from '@/components/ui/Button'
import { CardSkeleton } from '@/components/ui/Skeleton'
import { EmptyState } from '@/components/ui/EmptyState'
import { CoverImage } from '@/components/media/CoverImage'
import { Sentinel } from '@/components/filters/Sentinel'

interface BookPickerDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  title: string
  confirmLabel: string
  /** multi: any number of books; single: exactly one */
  mode: 'multi' | 'single'
  /** already members: shown disabled so they cannot be added twice */
  excludeIds?: ReadonlySet<string>
  onConfirm: (selected: BookDto[]) => void
  confirming?: boolean
  /** submit failure shown inside the dialog so the user can retry */
  error?: string
}

export function BookPickerDialog({
  open,
  onOpenChange,
  title,
  confirmLabel,
  mode,
  excludeIds,
  onConfirm,
  confirming,
  error,
}: BookPickerDialogProps) {
  const { t } = useTranslation('detail')
  const [text, setText] = useState('')
  const [search, setSearch] = useState('')
  const [selected, setSelected] = useState<Map<string, BookDto>>(new Map())

  if (useChanged([open]) && open) {
    setText('')
    setSearch('')
    setSelected(new Map())
  }

  useEffect(() => {
    const t = setTimeout(() => setSearch(text.trim()), 300)
    return () => clearTimeout(t)
  }, [text])

  const q = useInfiniteQuery({
    queryKey: ['book-picker', search],
    queryFn: ({ pageParam }) =>
      booksApi.list({ search: { fullTextSearch: search }, page: pageParam, size: 24, sort: ['metadata.titleSort'] }),
    initialPageParam: 0,
    getNextPageParam: (last) => (last.last ? undefined : last.number + 1),
    enabled: open,
    placeholderData: keepPreviousData,
  })

  const items = q.data?.pages.flatMap((p) => p.content) ?? []
  const { hasNextPage, isFetchingNextPage, isPlaceholderData, fetchNextPage } = q
  const loadMore = useCallback(() => {
    if (hasNextPage && !isFetchingNextPage && !isPlaceholderData) void fetchNextPage()
  }, [hasNextPage, isFetchingNextPage, isPlaceholderData, fetchNextPage])

  const toggle = (b: BookDto) => {
    setSelected((prev) => {
      const next = new Map(prev)
      if (next.has(b.id)) next.delete(b.id)
      else {
        if (mode === 'single') next.clear()
        next.set(b.id, b)
      }
      return next
    })
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange} title={title} size="lg">
      <div className="px-5 pt-4">
        <div className="relative">
          <MagnifyingGlass className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-ink-3" />
          <input
            type="search"
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder={t('picker.searchPlaceholder')}
            autoFocus
            className="h-9 w-full rounded-lg border border-line bg-surface pr-3 pl-9 text-base text-ink transition-colors placeholder:text-ink-3 focus:border-accent/70 focus:outline-none"
          />
        </div>
      </div>
      <div className="min-h-72 px-5 py-4">
        {q.isPending ? (
          <div className="grid grid-cols-3 gap-4 sm:grid-cols-4">
            {Array.from({ length: 8 }, (_, i) => (
              <CardSkeleton key={i} />
            ))}
          </div>
        ) : q.isLoadingError ? (
          <EmptyState
            title={t('empty.loadBooksFailed')}
            body={q.error.message}
            action={
              <Button variant="secondary" onClick={() => q.refetch()}>
                {t('common:action.retry')}
              </Button>
            }
          />
        ) : items.length === 0 ? (
          <EmptyState title={t('picker.noBooksFound')} body={search ? t('picker.tryDifferentSearch') : undefined} />
        ) : (
          <>
            <div className="grid grid-cols-3 gap-4 sm:grid-cols-4">
              {items.map((b) => {
                const excluded = excludeIds?.has(b.id) ?? false
                const isSelected = selected.has(b.id)
                const label = b.metadata.title || b.name
                return (
                  <button
                    key={b.id}
                    type="button"
                    disabled={excluded}
                    onClick={() => toggle(b)}
                    className={cn(
                      'group relative flex flex-col gap-1.5 rounded-lg text-left transition-opacity',
                      excluded ? 'cursor-not-allowed opacity-40' : 'cursor-pointer',
                    )}
                  >
                    <div
                      className={cn(
                        'relative rounded-lg transition-shadow',
                        isSelected && 'ring-2 ring-accent ring-offset-2 ring-offset-surface',
                      )}
                    >
                      <CoverImage src={urls.bookThumbnail(b.id)} alt={label} />
                      {isSelected && (
                        <span className="absolute top-1.5 right-1.5 flex size-5 items-center justify-center rounded-full bg-accent text-accent-ink">
                          <Check className="size-3.5" weight="bold" />
                        </span>
                      )}
                      {excluded && (
                        <span className="absolute bottom-1.5 left-1.5 rounded-full bg-black/60 px-1.5 text-[11px] text-white">
                          {t('picker.addedBadge')}
                        </span>
                      )}
                    </div>
                    <span className="line-clamp-2 text-xs font-medium text-ink">{label}</span>
                    <span className="-mt-1 truncate text-[11px] text-ink-3">
                      {b.seriesTitle} #{b.metadata.number}
                    </span>
                  </button>
                )
              })}
            </div>
            <Sentinel active={!!hasNextPage && !isPlaceholderData} onIntersect={loadMore} />
            {isFetchingNextPage && (
              <div className="mt-4 grid grid-cols-3 gap-4 sm:grid-cols-4">
                {Array.from({ length: 4 }, (_, i) => (
                  <CardSkeleton key={i} />
                ))}
              </div>
            )}
          </>
        )}
      </div>
      <div className="flex items-center justify-between gap-2 border-t border-line px-5 py-3.5">
        <p className="text-sm text-ink-3">
          {error ? (
            <span className="text-danger">{error}</span>
          ) : selected.size > 0 ? (
            t('selectedCount', { count: selected.size })
          ) : mode === 'single' ? (
            t('picker.pickOne')
          ) : (
            t('picker.noneSelected')
          )}
        </p>
        <div className="flex gap-2">
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            {t('common:action.cancel')}
          </Button>
          <Button
            variant="primary"
            loading={confirming}
            disabled={selected.size === 0 || (mode === 'single' && selected.size !== 1)}
            onClick={() => onConfirm([...selected.values()])}
          >
            {confirmLabel}
          </Button>
        </div>
      </div>
    </Dialog>
  )
}
