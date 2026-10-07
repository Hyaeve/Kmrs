import { useState, type ReactNode } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { ArrowCounterClockwise, Checks, CheckSquare, FolderPlus, PencilSimple, Sparkle, Trash } from '@phosphor-icons/react'
import type { SeriesDto } from '@/lib/api/types'
import { seriesApi } from '@/lib/api/series'
import { isAdmin, useAuthStore } from '@/lib/store/auth'
import { useKomfIntegration } from '@/lib/hooks/useKomfIntegration'
import { Menu, MenuItem, MenuSeparator } from '@/components/ui/Menu'
import { AddToCollectionDialog } from '@/components/browse/AddToCollectionDialog'
import { ConfirmDeleteFilesDialog } from '@/components/browse/ConfirmDeleteFilesDialog'
import { EditSeriesDialog } from '@/components/metadata/EditSeriesDialog'
import { KomfIdentifyDialog } from '@/components/metadata/KomfIdentifyDialog'
import { showToast } from '@/lib/store/toast'

/** Series actions for grid cards, plus the dialogs they open. */
export function SeriesCardMenu({ series, trigger, onSelect }: { series: SeriesDto; trigger: ReactNode; onSelect?: () => void }) {
  const { t } = useTranslation('media')
  const queryClient = useQueryClient()
  const user = useAuthStore((s) => s.user)
  const hasUnread = series.booksUnreadCount > 0
  const [addOpen, setAddOpen] = useState(false)
  const [editOpen, setEditOpen] = useState(false)
  const [identifyOpen, setIdentifyOpen] = useState(false)
  const [deleteOpen, setDeleteOpen] = useState(false)
  const komfReady = useKomfIntegration()

  const invalidate = () => {
    queryClient.invalidateQueries({ queryKey: ['series'] })
    queryClient.invalidateQueries({ queryKey: ['books'] })
    queryClient.invalidateQueries({ queryKey: ['dashboard'] })
    queryClient.invalidateQueries({ queryKey: ['readlists'] })
  }

  const markMutation = useMutation({
    mutationFn: (read: boolean) => (read ? seriesApi.markRead(series.id) : seriesApi.markUnread(series.id)),
    onSuccess: invalidate,
  })
  const deleteMutation = useMutation({
    mutationFn: () => seriesApi.deleteFile(series.id),
    onSuccess: () => {
      setDeleteOpen(false)
      invalidate()
    },
  })

  return (
    <>
      <Menu trigger={trigger}>
        {onSelect && (
          <MenuItem onSelect={onSelect}>
            <CheckSquare className="size-4" /> {t('menu.select')}
          </MenuItem>
        )}
        <MenuItem onSelect={() => markMutation.mutate(hasUnread)}>
          {hasUnread ? <Checks className="size-4" /> : <ArrowCounterClockwise className="size-4" />}
          {hasUnread ? t('menu.markAsRead') : t('menu.markAsUnread')}
        </MenuItem>
        <MenuItem onSelect={() => setAddOpen(true)}>
          <FolderPlus className="size-4" /> {t('menu.addToCollection')}
        </MenuItem>
        <MenuItem onSelect={() => setEditOpen(true)}>
          <PencilSimple className="size-4" /> {t('menu.editMetadata')}
        </MenuItem>
        {komfReady && (
          <MenuItem onSelect={() => setIdentifyOpen(true)}>
            <Sparkle className="size-4" /> {t('menu.identifyWithKomf')}
          </MenuItem>
        )}
        {isAdmin(user) && (
          <>
            <MenuSeparator />
            <MenuItem danger onSelect={() => setDeleteOpen(true)}>
              <Trash className="size-4" /> {t('menu.deleteFiles')}
            </MenuItem>
          </>
        )}
      </Menu>

      <AddToCollectionDialog open={addOpen} onOpenChange={setAddOpen} seriesIds={[series.id]} onDone={() => undefined} />
      <EditSeriesDialog open={editOpen} onClose={() => setEditOpen(false)} seriesIds={[series.id]} />
      {komfReady && (
        <KomfIdentifyDialog
          open={identifyOpen}
          onOpenChange={setIdentifyOpen}
          series={series}
          onIdentified={() => showToast(t('identifyQueued'))}
        />
      )}
      <ConfirmDeleteFilesDialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        count={1}
        kind="series"
        loading={deleteMutation.isPending}
        onConfirm={() => deleteMutation.mutate()}
      />
    </>
  )
}
