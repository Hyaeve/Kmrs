import { useState, type ReactNode } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import {
  ArrowCounterClockwise,
  ArrowsClockwise,
  Checks,
  CheckSquare,
  DownloadSimple,
  FileMagnifyingGlass,
  ListPlus,
  PencilSimple,
  Trash,
} from '@phosphor-icons/react'
import type { BookDto } from '@/lib/api/types'
import { booksApi } from '@/lib/api/books'
import { urls } from '@/lib/utils/urls'
import { canDownload, isAdmin, useAuthStore } from '@/lib/store/auth'
import { Menu, MenuItem, MenuSeparator } from '@/components/ui/Menu'
import { AddToReadListDialog } from '@/components/browse/AddToReadListDialog'
import { ConfirmDeleteFilesDialog } from '@/components/browse/ConfirmDeleteFilesDialog'
import { EditBooksDialog } from '@/components/metadata/EditBooksDialog'
import { showToast } from '@/lib/store/toast'

interface BookCardMenuProps {
  book: BookDto
  trigger: ReactNode
  /** leading navigation items; each card links one destination and offers the other here */
  navItem?: ReactNode
  /** when set, offers entering selection mode with this book */
  onSelect?: () => void
}

/** Book actions shared by horizontal and grid cards, plus the dialogs they open. */
export function BookCardMenu({ book, trigger, navItem, onSelect }: BookCardMenuProps) {
  const { t } = useTranslation('media')
  const queryClient = useQueryClient()
  const user = useAuthStore((s) => s.user)
  const completed = book.readProgress?.completed ?? false
  const [addOpen, setAddOpen] = useState(false)
  const [editOpen, setEditOpen] = useState(false)
  const [deleteOpen, setDeleteOpen] = useState(false)

  const invalidate = () => {
    queryClient.invalidateQueries({ queryKey: ['books'] })
    queryClient.invalidateQueries({ queryKey: ['series'] })
    queryClient.invalidateQueries({ queryKey: ['dashboard'] })
    queryClient.invalidateQueries({ queryKey: ['readlists'] })
  }

  const markMutation = useMutation({
    mutationFn: (read: boolean) => (read ? booksApi.markRead(book.id) : booksApi.markUnread(book.id)),
    onSuccess: invalidate,
  })
  const deleteMutation = useMutation({
    mutationFn: () => booksApi.deleteFile(book.id),
    onSuccess: () => {
      setDeleteOpen(false)
      invalidate()
    },
    onError: (e) => showToast(e instanceof Error ? e.message : t('detail:toast.deleteFileFailed')),
  })
  const analyzeMutation = useMutation({
    mutationFn: () => booksApi.analyze(book.id),
    onSuccess: () => showToast(t('detail:toast.analysisQueued')),
    onError: (e) => showToast(e instanceof Error ? e.message : t('detail:toast.queueAnalysisFailed')),
  })
  const refreshMutation = useMutation({
    mutationFn: () => booksApi.refreshMetadata(book.id),
    onSuccess: () => showToast(t('detail:toast.refreshQueued')),
    onError: (e) => showToast(e instanceof Error ? e.message : t('detail:toast.queueRefreshFailed')),
  })

  return (
    <>
      <Menu trigger={trigger}>
        {navItem}
        {onSelect && (
          <MenuItem onSelect={onSelect}>
            <CheckSquare className="size-4" /> {t('menu.select')}
          </MenuItem>
        )}
        {!completed && (
          <MenuItem onSelect={() => markMutation.mutate(true)}>
            <Checks className="size-4" /> {t('menu.markAsRead')}
          </MenuItem>
        )}
        {book.readProgress && (
          <MenuItem onSelect={() => markMutation.mutate(false)}>
            <ArrowCounterClockwise className="size-4" /> {t('menu.markAsUnread')}
          </MenuItem>
        )}
        <MenuItem onSelect={() => setAddOpen(true)}>
          <ListPlus className="size-4" /> {t('menu.addToReadList')}
        </MenuItem>
        {canDownload(user) && (
          <MenuItem
            onSelect={() => {
              const a = document.createElement('a')
              a.href = urls.bookFile(book.id)
              a.download = ''
              a.click()
            }}
          >
            <DownloadSimple className="size-4" /> {t('menu.download')}
          </MenuItem>
        )}
        {isAdmin(user) && (
          <>
            <MenuSeparator />
            <MenuItem onSelect={() => analyzeMutation.mutate()} disabled={analyzeMutation.isPending}>
              <FileMagnifyingGlass className="size-4" /> {t('detail:menu.analyze')}
            </MenuItem>
            <MenuItem onSelect={() => refreshMutation.mutate()} disabled={refreshMutation.isPending}>
              <ArrowsClockwise className="size-4" /> {t('detail:menu.refreshMetadata')}
            </MenuItem>
            <MenuItem onSelect={() => setEditOpen(true)}>
              <PencilSimple className="size-4" /> {t('menu.editMetadata')}
            </MenuItem>
            <MenuItem danger onSelect={() => setDeleteOpen(true)}>
              <Trash className="size-4" /> {t('menu.deleteFile')}
            </MenuItem>
          </>
        )}
      </Menu>

      <AddToReadListDialog open={addOpen} onOpenChange={setAddOpen} bookIds={[book.id]} onDone={() => undefined} />
      <EditBooksDialog open={editOpen} onClose={() => setEditOpen(false)} bookIds={[book.id]} />
      <ConfirmDeleteFilesDialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        count={1}
        kind="book"
        loading={deleteMutation.isPending}
        onConfirm={() => deleteMutation.mutate()}
      />
    </>
  )
}
