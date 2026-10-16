import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Plus } from '@phosphor-icons/react'
import { readlistsApi } from '@/lib/api/collections'
import { booksApi } from '@/lib/api/books'
import type { ReadListDto } from '@/lib/api/types'
import { useChanged } from '@/lib/hooks/useChanged'
import { Dialog } from '@/components/ui/Dialog'
import { TextField } from '@/components/ui/TextField'
import { Button } from '@/components/ui/Button'
import { Switch } from '@/components/ui/Switch'
import { Skeleton } from '@/components/ui/Skeleton'

interface AddToReadListDialogProps {
  bookId: string
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function AddToReadListDialog({ bookId, open, onOpenChange }: AddToReadListDialogProps) {
  const { t } = useTranslation('detail')
  const queryClient = useQueryClient()
  const [newName, setNewName] = useState('')
  const [creating, setCreating] = useState(false)

  if (useChanged([open]) && open) {
    setNewName('')
    setCreating(false)
  }

  const listsQuery = useQuery({
    queryKey: ['readlists', 'all'],
    queryFn: () => readlistsApi.list({ size: 100 }),
    enabled: open,
  })
  const membershipQuery = useQuery({
    queryKey: ['readlists', 'book', bookId],
    queryFn: () => booksApi.readlists(bookId),
    enabled: open,
  })

  const toggleMutation = useMutation({
    mutationFn: ({ list, add }: { list: ReadListDto; add: boolean }) =>
      readlistsApi.update(list.id, {
        bookIds: add ? [...list.bookIds, bookId] : list.bookIds.filter((id) => id !== bookId),
      }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['readlists'] }),
  })

  const createMutation = useMutation({
    mutationFn: () => readlistsApi.create({ name: newName.trim(), summary: '', ordered: false, bookIds: [bookId] }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['readlists'] })
      setNewName('')
      setCreating(false)
    },
  })

  const lists = listsQuery.data?.content ?? []
  const memberIds = new Set((membershipQuery.data ?? []).map((l) => l.id))
  // with no existing lists, creating is the only possible action
  const creatingOpen = creating || (listsQuery.isSuccess && lists.length === 0)

  return (
    <Dialog open={open} onOpenChange={onOpenChange} title={t('addToReadList')} size="sm">
      <div className="py-2">
        {listsQuery.isPending ? (
          <div className="space-y-3 px-5 py-2">
            <Skeleton className="h-8 w-full" />
            <Skeleton className="h-8 w-full" />
            <Skeleton className="h-8 w-2/3" />
          </div>
        ) : lists.length === 0 ? (
          <p className="px-5 py-3 text-sm text-ink-3">{t('addToReadListDialog.empty')}</p>
        ) : (
          lists.map((list) => (
            <div key={list.id} className="flex items-center justify-between gap-3 px-5 py-2">
              <div className="min-w-0">
                <p className="truncate text-sm text-ink">{list.name}</p>
                <p className="text-xs text-ink-3">{t('bookCount', { count: list.bookIds.length })}</p>
              </div>
              <Switch
                checked={memberIds.has(list.id)}
                onCheckedChange={(add) => toggleMutation.mutate({ list, add })}
                label={list.name}
                disabled={toggleMutation.isPending}
              />
            </div>
          ))
        )}
      </div>
      <div className="border-t border-line px-5 py-4">
        {creatingOpen ? (
          <form
            onSubmit={(e) => {
              e.preventDefault()
              if (newName.trim()) createMutation.mutate()
            }}
            className="flex items-end gap-2"
          >
            <TextField
              label={t('browse:addToReadList.newLabel')}
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              placeholder={t('nameLabel')}
              className="flex-1"
              autoFocus
            />
            <Button type="submit" variant="primary" loading={createMutation.isPending} disabled={!newName.trim()}>
              {t('common:action.create')}
            </Button>
            {lists.length > 0 && (
              <Button type="button" variant="ghost" onClick={() => setCreating(false)}>
                {t('common:action.cancel')}
              </Button>
            )}
          </form>
        ) : (
          <button
            type="button"
            onClick={() => setCreating(true)}
            className="flex w-full cursor-pointer items-center gap-2 rounded-lg px-3 py-2 text-sm text-ink-2 transition-colors hover:bg-raised hover:text-ink"
          >
            <Plus className="size-4" />
            {t('browse:addToReadList.newLabel')}
          </button>
        )}
      </div>
    </Dialog>
  )
}
