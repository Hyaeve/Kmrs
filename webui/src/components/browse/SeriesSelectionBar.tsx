import { useEffect, useMemo, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Checks, DotsThreeVertical, FolderPlus, PencilSimple, PlugsConnected, Trash } from '@phosphor-icons/react'
import { seriesApi } from '@/lib/api/series'
import { komfApi } from '@/lib/api/komf'
import type { SeriesDto } from '@/lib/api/types'
import { isAdmin, useAuthStore } from '@/lib/store/auth'
import { useKomfIntegration } from '@/lib/hooks/useKomfIntegration'
import { trackKomfJobs } from '@/lib/store/komfJobs'
import { Button } from '@/components/ui/Button'
import { IconButton } from '@/components/ui/IconButton'
import { Menu, MenuItem, MenuSeparator } from '@/components/ui/Menu'
import type { Selection } from '@/components/selection/useSelection'
import { useBatchRun } from '@/components/selection/useBatchRun'
import { SelectionBar } from './SelectionBar'
import { AddToCollectionDialog } from './AddToCollectionDialog'
import { ConfirmDeleteFilesDialog } from './ConfirmDeleteFilesDialog'
import { EditSeriesDialog } from '@/components/metadata/EditSeriesDialog'

interface SeriesSelectionBarProps {
  selection: Selection
  /** series currently loaded in the grid; "Select all" covers these */
  items: SeriesDto[]
}

export function SeriesSelectionBar({ selection, items }: SeriesSelectionBarProps) {
  const { t } = useTranslation('browse')
  const queryClient = useQueryClient()
  const admin = isAdmin(useAuthStore((s) => s.user))
  const komfReady = useKomfIntegration()
  const { state, run, setResult, reset } = useBatchRun()
  const [addOpen, setAddOpen] = useState(false)
  const [editOpen, setEditOpen] = useState(false)
  const [deleteOpen, setDeleteOpen] = useState(false)
  const busy = state.status === 'running'
  const ids = selection.ids
  const loadedIds = useMemo(() => items.map((s) => s.id), [items])
  const byId = useMemo(() => new Map(items.map((s) => [s.id, s])), [items])

  const invalidate = () => {
    queryClient.invalidateQueries({ queryKey: ['series'] })
    queryClient.invalidateQueries({ queryKey: ['books'] })
    queryClient.invalidateQueries({ queryKey: ['dashboard'] })
    queryClient.invalidateQueries({ queryKey: ['readlists'] })
  }

  // after a successful run the bar lingers briefly so the result message reads, then the selection clears
  useEffect(() => {
    if (state.status !== 'success') return
    const t = setTimeout(() => {
      selection.clear()
      reset()
    }, 2200)
    return () => clearTimeout(t)
  }, [state.status, selection, reset])

  const mark = (read: boolean) => {
    void run(ids, (id) => (read ? seriesApi.markRead(id) : seriesApi.markUnread(id)), {
      success: { key: 'browse:selection.markSeriesAs', context: read ? 'read' : 'unread' },
      failure: { key: 'browse:selection.markSeriesFailed' },
    }).then((failed) => {
      if (failed < ids.length) invalidate()
    })
  }

  const deleteFiles = () => {
    setDeleteOpen(false)
    void run(ids, (id) => seriesApi.deleteFile(id), {
      success: { key: 'browse:selection.deleteSeriesFiles' },
      failure: { key: 'browse:selection.deleteFilesFailed' },
    }).then((failed) => {
      if (failed < ids.length) invalidate()
    })
  }

  // jobs are tracked in one shot after the loop so the aggregate event stream
  // opens once per batch instead of reopening for every queued job. komf PATCHes
  // the metadata asynchronously once the jobs run; the SSE SeriesChanged
  // invalidation picks the results up, so no query invalidation here
  const matchKomf = () => {
    const matched: { id: string; label: string }[] = []
    void run(
      ids,
      async (id) => {
        const series = byId.get(id)
        if (!series) throw new Error(`series ${id} is not loaded`)
        const job = await komfApi.matchSeries(series.libraryId, id)
        matched.push({ id: job.id, label: series.metadata.title || series.name })
      },
      {
        success: { key: 'browse:selection.matchKomfQueued' },
        failure: { key: 'browse:selection.matchKomfFailed' },
      },
    ).then(() => {
      if (matched.length > 0) trackKomfJobs(matched)
    })
  }

  return (
    <>
      <SelectionBar
        count={selection.size}
        loaded={loadedIds.length}
        state={state}
        onSelectAll={() => selection.selectAll(loadedIds)}
        onClear={() => {
          selection.clear()
          reset()
        }}
        onDismiss={reset}
      >
        <Button variant="ghost" size="sm" disabled={busy} onClick={() => mark(true)}>
          <Checks className="size-4" />
          {t('selection.markRead')}
        </Button>
        <Button variant="ghost" size="sm" disabled={busy} onClick={() => mark(false)}>
          {t('selection.markUnread')}
        </Button>
        <Menu
          side="top"
          trigger={
            <IconButton label={t('selection.moreActions')} className="size-8">
              <DotsThreeVertical className="size-4.5" />
            </IconButton>
          }
        >
          <MenuItem onSelect={() => setAddOpen(true)}>
            <FolderPlus className="size-4" /> {t('selection.addToCollection')}
          </MenuItem>
          <MenuItem onSelect={() => setEditOpen(true)}>
            <PencilSimple className="size-4" /> {t('selection.editMetadata')}
          </MenuItem>
          {komfReady && (
            <MenuItem onSelect={matchKomf}>
              <PlugsConnected className="size-4" /> {t('selection.matchKomf')}
            </MenuItem>
          )}
          {admin && (
            <>
              <MenuSeparator />
              <MenuItem danger onSelect={() => setDeleteOpen(true)}>
                <Trash className="size-4" /> {t('selection.deleteFiles')}
              </MenuItem>
            </>
          )}
        </Menu>
      </SelectionBar>

      <AddToCollectionDialog
        open={addOpen}
        onOpenChange={setAddOpen}
        seriesIds={ids}
        onDone={(ok, message) => setResult(ok ? 'success' : 'error', message)}
      />
      <EditSeriesDialog open={editOpen} onClose={() => setEditOpen(false)} seriesIds={ids} />
      <ConfirmDeleteFilesDialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        count={ids.length}
        kind="series"
        loading={busy}
        onConfirm={deleteFiles}
      />
    </>
  )
}
