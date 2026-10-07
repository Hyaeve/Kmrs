import { useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { Dialog } from '@/components/ui/Dialog'
import { urls } from '@/lib/utils/urls'
import { cn } from '@/lib/utils/cn'

interface ThumbnailExplorerProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  bookId: string
  pagesCount: number
  currentPage: number
  onGoToPage: (page: number) => void
}

export function ThumbnailExplorer({
  open,
  onOpenChange,
  bookId,
  pagesCount,
  currentPage,
  onGoToPage,
}: ThumbnailExplorerProps) {
  const { t } = useTranslation('reader')
  const currentRef = useRef<HTMLButtonElement>(null)

  useEffect(() => {
    if (!open) return
    const t = window.setTimeout(() => currentRef.current?.scrollIntoView({ block: 'center' }), 60)
    return () => window.clearTimeout(t)
  }, [open])

  return (
    <Dialog open={open} onOpenChange={onOpenChange} title={t('chrome.pages')} size="lg">
      <div className="grid grid-cols-3 gap-3 p-4 sm:grid-cols-5">
        {Array.from({ length: pagesCount }, (_, i) => i + 1).map((n) => (
          <button
            key={n}
            ref={n === currentPage ? currentRef : undefined}
            type="button"
            onClick={() => {
              onGoToPage(n)
              onOpenChange(false)
            }}
            className={cn(
              'relative cursor-pointer overflow-hidden rounded-lg border transition-colors',
              n === currentPage ? 'border-accent ring-2 ring-accent/50' : 'border-line hover:border-line-strong',
            )}
          >
            <img
              src={urls.bookPageThumbnail(bookId, n)}
              alt={t('pageAlt', { page: n })}
              loading="lazy"
              draggable={false}
              className="aspect-[0.7071] w-full bg-raised object-cover"
            />
            <span className="absolute right-1 bottom-1 rounded bg-black/75 px-1.5 py-0.5 font-mono text-[11px] text-white">
              {n}
            </span>
          </button>
        ))}
      </div>
    </Dialog>
  )
}
