import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import {
  ArrowCounterClockwise,
  ArrowsClockwise,
  ChartBar,
  DotsThreeVertical,
  MagnifyingGlassPlus,
  PlugsConnected,
  Scan,
  TrashSimple,
} from '@phosphor-icons/react'
import { librariesApi } from '@/lib/api/libraries'
import { komfApi } from '@/lib/api/komf'
import type { LibraryDto } from '@/lib/api/types'
import { showToast } from '@/lib/store/toast'
import { useKomfIntegration } from '@/lib/hooks/useKomfIntegration'
import { IconButton } from '@/components/ui/IconButton'
import { Menu, MenuItem, MenuSeparator } from '@/components/ui/Menu'
import { KomfResetDialog } from '@/components/metadata/KomfResetDialog'

/** Per-library admin actions (scan family) attached to each sidebar library row. */
export function SidebarLibraryMenu({ library }: { library: LibraryDto }) {
  const { t } = useTranslation('layout')
  const queryClient = useQueryClient()
  const komfReady = useKomfIntegration()
  const [komfResetOpen, setKomfResetOpen] = useState(false)

  const handlers = (queued: string, failed: string) => ({
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['libraries'] })
      showToast(queued)
    },
    onError: (e: unknown) => showToast(e instanceof Error ? e.message : failed),
  })

  const scan = useMutation({
    mutationFn: (deep: boolean) => librariesApi.scan(library.id, deep),
    ...handlers(t('libraryMenu.scanQueued'), t('libraryMenu.scanFailed')),
  })
  const analyze = useMutation({
    mutationFn: () => librariesApi.analyze(library.id),
    ...handlers(t('libraryMenu.analyzeQueued'), t('libraryMenu.analyzeFailed')),
  })
  const refresh = useMutation({
    mutationFn: () => librariesApi.refreshMetadata(library.id),
    ...handlers(t('libraryMenu.refreshQueued'), t('libraryMenu.refreshFailed')),
  })
  const emptyTrash = useMutation({
    mutationFn: () => librariesApi.emptyTrash(library.id),
    ...handlers(t('libraryMenu.emptyTrashQueued'), t('libraryMenu.emptyTrashFailed')),
  })
  // a library match is fire-and-forget on komf's side (no job id), so it only gets a toast
  const komfMatch = useMutation({
    mutationFn: () => komfApi.matchLibrary(library.id),
    onSuccess: () => showToast(t('libraryMenu.matchQueued')),
    onError: (e) => showToast(e instanceof Error ? e.message : t('libraryMenu.matchFailed')),
  })
  const komfReset = useMutation({
    mutationFn: () => komfApi.resetLibrary(library.id),
    onSuccess: () => {
      setKomfResetOpen(false)
      queryClient.invalidateQueries({ queryKey: ['series'] })
      queryClient.invalidateQueries({ queryKey: ['books'] })
      showToast(t('libraryMenu.metadataReset'))
    },
    onError: (e) => showToast(e instanceof Error ? e.message : t('libraryMenu.metadataResetFailed')),
  })
  const busy =
    scan.isPending || analyze.isPending || refresh.isPending || emptyTrash.isPending || komfMatch.isPending || komfReset.isPending

  return (
    <>
      <Menu
        side="right"
        align="start"
        trigger={
          <IconButton
            label={t('libraryMenu.actions', { name: library.name })}
            className="size-6 rounded-md opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100 data-[state=open]:opacity-100 [@media(pointer:coarse)]:opacity-60 [&_svg]:size-3.5"
          >
            <DotsThreeVertical />
          </IconButton>
        }
      >
        <MenuItem disabled={busy} onSelect={() => scan.mutate(false)}>
          <Scan className="size-4" />
          {t('libraryMenu.scan')}
        </MenuItem>
        <MenuItem disabled={busy} onSelect={() => scan.mutate(true)}>
          <MagnifyingGlassPlus className="size-4" />
          {t('libraryMenu.scanDeep')}
        </MenuItem>
        <MenuItem disabled={busy} onSelect={() => analyze.mutate()}>
          <ChartBar className="size-4" />
          {t('libraryMenu.analyze')}
        </MenuItem>
        <MenuItem disabled={busy} onSelect={() => refresh.mutate()}>
          <ArrowsClockwise className="size-4" />
          {t('libraryMenu.refreshMetadata')}
        </MenuItem>
        <MenuItem disabled={busy} onSelect={() => emptyTrash.mutate()}>
          <TrashSimple className="size-4" />
          {t('libraryMenu.emptyTrash')}
        </MenuItem>
        {komfReady && (
          <>
            <MenuSeparator />
            <MenuItem disabled={busy} onSelect={() => komfMatch.mutate()}>
              <PlugsConnected className="size-4" />
              {t('libraryMenu.matchKomf')}
            </MenuItem>
            <MenuItem disabled={busy} onSelect={() => setKomfResetOpen(true)}>
              <ArrowCounterClockwise className="size-4" />
              {t('libraryMenu.resetKomf')}
            </MenuItem>
          </>
        )}
      </Menu>
      {komfReady && (
        <KomfResetDialog
          open={komfResetOpen}
          onOpenChange={setKomfResetOpen}
          name={library.name}
          bodyKey="reset.libraryBody"
          loading={komfReset.isPending}
          onConfirm={() => komfReset.mutate()}
        />
      )}
    </>
  )
}
