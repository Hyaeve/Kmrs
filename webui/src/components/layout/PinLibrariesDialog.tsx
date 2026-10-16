import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { PushPin } from '@phosphor-icons/react'
import { librariesApi } from '@/lib/api/libraries'
import { cn } from '@/lib/utils/cn'
import { usePinnedLibraries, useSetPinnedLibraries } from '@/lib/store/clientSettings'
import { Dialog } from '@/components/ui/Dialog'
import { Switch } from '@/components/ui/Switch'
import { Button } from '@/components/ui/Button'
import { Skeleton } from '@/components/ui/Skeleton'

interface PinLibrariesDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function PinLibrariesDialog({ open, onOpenChange }: PinLibrariesDialogProps) {
  const { t } = useTranslation('layout')
  const { data: libraries, isLoading } = useQuery({ queryKey: ['libraries'], queryFn: librariesApi.list })
  const { pinned } = usePinnedLibraries()
  const { setPinned, resetPinned, isPending } = useSetPinnedLibraries()

  const pinnedIds = pinned ?? []
  const pinnedSet = new Set(pinnedIds)
  const pinnedLibs = pinnedIds.map((id) => libraries?.find((l) => l.id === id)).filter((l) => !!l)
  const otherLibs = (libraries ?? []).filter((l) => !pinnedSet.has(l.id))

  const toggle = (libraryId: string, pin: boolean) =>
    setPinned(pin ? [...pinnedIds, libraryId] : pinnedIds.filter((id) => id !== libraryId))

  const renderRow = (lib: { id: string; name: string }, isPinned: boolean) => (
    <div key={lib.id} className="flex items-center gap-3 px-5 py-2.5">
      <PushPin
        weight={isPinned ? 'fill' : 'regular'}
        className={cn('size-4 shrink-0', isPinned ? 'text-accent' : 'text-ink-3')}
      />
      <p className="min-w-0 flex-1 truncate text-sm text-ink">{lib.name}</p>
      <Switch checked={isPinned} onCheckedChange={(pin) => toggle(lib.id, pin)} label={lib.name} disabled={isPending} />
    </div>
  )

  return (
    <Dialog open={open} onOpenChange={onOpenChange} title={t('pinDialog.title')} size="sm">
      <div className="py-2">
        {isLoading ? (
          <div className="space-y-3 px-5 py-2">
            <Skeleton className="h-7 w-full" />
            <Skeleton className="h-7 w-full" />
            <Skeleton className="h-7 w-2/3" />
          </div>
        ) : libraries?.length ? (
          <>
            {pinnedLibs.length > 0 && (
              <>
                <p className="px-5 pt-1.5 pb-1 text-[11px] font-medium tracking-wide text-ink-3 uppercase">
                  {t('nav.pinned')}
                </p>
                {pinnedLibs.map((lib) => renderRow(lib, true))}
              </>
            )}
            {otherLibs.length > 0 && (
              <>
                <p className="px-5 pt-1.5 pb-1 text-[11px] font-medium tracking-wide text-ink-3 uppercase">
                  {t('nav.libraries')}
                </p>
                {otherLibs.map((lib) => renderRow(lib, false))}
              </>
            )}
          </>
        ) : (
          <p className="px-5 py-3 text-sm text-ink-3">{t('nav.noLibraries')}</p>
        )}
      </div>
      <div className="flex items-center justify-between gap-3 border-t border-line px-5 py-3.5">
        <p className="text-xs leading-relaxed text-ink-3">{t('pinDialog.hint')}</p>
        {pinned !== undefined && (
          <Button variant="ghost" size="sm" onClick={() => resetPinned()} disabled={isPending}>
            {t('pinDialog.reset')}
          </Button>
        )}
      </div>
    </Dialog>
  )
}
