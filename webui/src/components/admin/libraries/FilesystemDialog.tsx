import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { ArrowUp, Folder, WarningCircle } from '@phosphor-icons/react'
import { filesystemApi } from '@/lib/api/settings'
import { useChanged } from '@/lib/hooks/useChanged'
import { Dialog } from '@/components/ui/Dialog'
import { Button } from '@/components/ui/Button'
import { IconButton } from '@/components/ui/IconButton'
import { Skeleton } from '@/components/ui/Skeleton'
import { Tooltip } from '@/components/ui/Tooltip'

interface FilesystemDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** absolute path to start from; omitted = filesystem root */
  initialPath?: string
  onSelect: (path: string) => void
}

export function FilesystemDialog({ open, onOpenChange, initialPath, onSelect }: FilesystemDialogProps) {
  const { t } = useTranslation('admin-maintenance')
  const [path, setPath] = useState<string | undefined>(initialPath)

  if (useChanged([open, initialPath]) && open) setPath(initialPath || undefined)

  const q = useQuery({
    queryKey: ['admin', 'filesystem', path ?? ''],
    queryFn: () => filesystemApi.browse(path),
    enabled: open,
    retry: false,
  })
  const listing = q.data

  return (
    <Dialog open={open} onOpenChange={onOpenChange} title={t('filesystem.title')}>
      <div className="flex items-center gap-2 border-b border-line px-5 py-3">
        <IconButton label={t('filesystem.up')} disabled={!listing?.parent} onClick={() => setPath(listing?.parent)}>
          <ArrowUp className="size-4" />
        </IconButton>
        <Tooltip content={path ?? '/'}>
          <span className="min-w-0 flex-1 truncate font-mono text-[13px] text-ink-2">{path ?? '/'}</span>
        </Tooltip>
      </div>
      <div className="h-80 overflow-y-auto px-3 py-2">
        {q.isPending ? (
          <div className="space-y-1">
            {Array.from({ length: 7 }, (_, i) => (
              <Skeleton key={i} className="h-9" />
            ))}
          </div>
        ) : q.isLoadingError ? (
          <div className="flex h-full flex-col items-center justify-center gap-2 px-6 text-center">
            <WarningCircle className="size-8 text-danger" />
            <p className="text-sm text-ink-2">{q.error instanceof Error ? q.error.message : t('filesystem.listError')}</p>
            <Button type="button" size="sm" onClick={() => setPath(undefined)}>
              {t('filesystem.backToRoot')}
            </Button>
          </div>
        ) : listing && listing.directories.length === 0 ? (
          <div className="flex h-full items-center justify-center">
            <p className="text-sm text-ink-3">{t('filesystem.empty')}</p>
          </div>
        ) : (
          listing?.directories.map((d) => (
            <button
              key={d.path}
              type="button"
              onClick={() => setPath(d.path)}
              className="flex w-full cursor-pointer items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-sm text-ink transition-colors hover:bg-raised"
            >
              <Folder className="size-4 shrink-0 text-ink-3" />
              <span className="min-w-0 truncate">{d.name}</span>
            </button>
          ))
        )}
      </div>
      <div className="flex justify-end gap-2 border-t border-line px-5 py-3.5">
        <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
          {t('common:action.cancel')}
        </Button>
        <Button
          type="button"
          variant="primary"
          disabled={!path}
          onClick={() => {
            if (!path) return
            onSelect(path)
            onOpenChange(false)
          }}
        >
          {t('filesystem.select')}
        </Button>
      </div>
    </Dialog>
  )
}
