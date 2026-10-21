import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Books, CaretDown, MagnifyingGlassPlus, Plus, Scan, TrashSimple, WarningCircle } from '@phosphor-icons/react'
import { librariesApi } from '@/lib/api/libraries'
import type { LibraryDto } from '@/lib/api/types'
import { useDocumentTitle } from '@/lib/hooks/useDocumentTitle'
import { showToast } from '@/lib/store/toast'
import { Button } from '@/components/ui/Button'
import { Dialog } from '@/components/ui/Dialog'
import { EmptyState } from '@/components/ui/EmptyState'
import { Menu, MenuItem, MenuSeparator } from '@/components/ui/Menu'
import { PageHeader } from '@/components/ui/PageHeader'
import { Skeleton } from '@/components/ui/Skeleton'
import { DeleteLibraryDialog } from '@/components/admin/libraries/DeleteLibraryDialog'
import { LibraryCard } from '@/components/admin/libraries/LibraryCard'
import { LibraryDialog } from '@/components/admin/libraries/LibraryDialog'

const GRID_STYLE = { gridTemplateColumns: 'repeat(auto-fill, minmax(300px, 1fr))' }

export function AdminLibrariesPage() {
  const { t } = useTranslation('admin-maintenance')
  const [addOpen, setAddOpen] = useState(false)
  const [editing, setEditing] = useState<LibraryDto | null>(null)
  const [deleting, setDeleting] = useState<LibraryDto | null>(null)
  const [emptyTrashOpen, setEmptyTrashOpen] = useState(false)

  useDocumentTitle(t('layout:nav.libraries'))

  const q = useQuery({ queryKey: ['libraries'], queryFn: librariesApi.list })
  const libraries = q.data ?? []

  const queryClient = useQueryClient()
  const scanAll = useMutation({
    mutationFn: (deep: boolean) => Promise.all(libraries.map((l) => librariesApi.scan(l.id, deep))),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['libraries'] })
      showToast(t('layout:libraryMenu.scanQueued'))
    },
    onError: (e: unknown) => showToast(e instanceof Error ? e.message : t('layout:libraryMenu.scanFailed')),
  })
  const emptyTrashAll = useMutation({
    mutationFn: () => Promise.all(libraries.map((l) => librariesApi.emptyTrash(l.id))),
    onSuccess: () => {
      setEmptyTrashOpen(false)
      queryClient.invalidateQueries({ queryKey: ['libraries'] })
      showToast(t('layout:libraryMenu.emptyTrashQueued'))
    },
    onError: (e: unknown) => showToast(e instanceof Error ? e.message : t('layout:libraryMenu.emptyTrashFailed')),
  })

  return (
    <div>
      <PageHeader
        title={t('layout:nav.libraries')}
        subtitle={q.data ? t('libraries.subtitle', { count: q.data.length }) : undefined}
        actions={
          <>
            {libraries.length > 0 && (
              <Menu
                align="end"
                trigger={
                  <Button variant="secondary" loading={scanAll.isPending}>
                    {t('layout:libraryMenu.allLibraries')}
                    <CaretDown className="size-3.5" />
                  </Button>
                }
              >
                <MenuItem onSelect={() => scanAll.mutate(false)}>
                  <Scan className="size-4" />
                  {t('layout:libraryMenu.scan')}
                </MenuItem>
                <MenuItem onSelect={() => scanAll.mutate(true)}>
                  <MagnifyingGlassPlus className="size-4" />
                  {t('layout:libraryMenu.scanDeep')}
                </MenuItem>
                <MenuSeparator />
                <MenuItem onSelect={() => setEmptyTrashOpen(true)}>
                  <TrashSimple className="size-4" />
                  {t('layout:libraryMenu.emptyTrash')}
                </MenuItem>
              </Menu>
            )}
            <Button variant="primary" onClick={() => setAddOpen(true)}>
              <Plus className="size-4" />
              {t('libraries.add')}
            </Button>
          </>
        }
      />
      {q.isPending ? (
        <div className="grid gap-4" style={GRID_STYLE}>
          {Array.from({ length: 3 }, (_, i) => (
            <Skeleton key={i} className="h-36 rounded-xl" />
          ))}
        </div>
      ) : q.isLoadingError ? (
        <EmptyState
          icon={<WarningCircle />}
          title={t('libraries.loadError')}
          body={q.error instanceof Error ? q.error.message : t('errorFallback')}
          action={<Button onClick={() => q.refetch()}>{t('common:action.retry')}</Button>}
        />
      ) : libraries.length === 0 ? (
        <EmptyState
          icon={<Books />}
          title={t('layout:nav.noLibraries')}
          body={t('libraries.emptyBody')}
          action={
            <Button variant="primary" onClick={() => setAddOpen(true)}>
              <Plus className="size-4" />
              {t('libraries.add')}
            </Button>
          }
        />
      ) : (
        <div className="grid gap-4" style={GRID_STYLE}>
          {libraries.map((lib) => (
            <LibraryCard key={lib.id} library={lib} onEdit={setEditing} onDelete={setDeleting} />
          ))}
        </div>
      )}

      <LibraryDialog open={addOpen} onOpenChange={setAddOpen} />
      {editing && (
        <LibraryDialog
          key={editing.id}
          open
          onOpenChange={(o) => {
            if (!o) setEditing(null)
          }}
          library={editing}
        />
      )}
      <DeleteLibraryDialog
        library={deleting}
        onOpenChange={(o) => {
          if (!o) setDeleting(null)
        }}
      />
      <Dialog open={emptyTrashOpen} onOpenChange={setEmptyTrashOpen} title={t('layout:libraryMenu.emptyTrash')} size="sm">
        <div className="px-5 py-4">
          <p className="text-sm text-ink-2">{t('layout:libraryMenu.emptyTrashAllBody')}</p>
        </div>
        <div className="flex justify-end gap-2 border-t border-line px-5 py-3.5">
          <Button variant="ghost" onClick={() => setEmptyTrashOpen(false)}>
            {t('common:action.cancel')}
          </Button>
          <Button variant="danger" loading={emptyTrashAll.isPending} onClick={() => emptyTrashAll.mutate()}>
            {t('layout:libraryMenu.emptyTrash')}
          </Button>
        </div>
      </Dialog>
    </div>
  )
}
