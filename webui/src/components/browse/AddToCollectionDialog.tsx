import { useMemo, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Plus } from '@phosphor-icons/react'
import { collectionsApi } from '@/lib/api/collections'
import { useChanged } from '@/lib/hooks/useChanged'
import { Dialog } from '@/components/ui/Dialog'
import { TextField } from '@/components/ui/TextField'
import { Button } from '@/components/ui/Button'
import { Skeleton } from '@/components/ui/Skeleton'

interface AddToCollectionDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  seriesIds: string[]
  /** reports the outcome so the caller's selection bar can show it */
  onDone: (ok: boolean, message: string) => void
}

export function AddToCollectionDialog({ open, onOpenChange, seriesIds, onDone }: AddToCollectionDialogProps) {
  const { t } = useTranslation('browse')
  const queryClient = useQueryClient()
  const [filter, setFilter] = useState('')
  const [newName, setNewName] = useState('')
  const [creating, setCreating] = useState(false)

  if (useChanged([open]) && open) {
    setFilter('')
    setNewName('')
    setCreating(false)
  }

  const listQuery = useQuery({
    queryKey: ['collections', 'all'],
    queryFn: () => collectionsApi.list({ size: 100 }),
    enabled: open,
  })

  const finish = (ok: boolean, message: string) => {
    onOpenChange(false)
    onDone(ok, message)
  }

  const addMutation = useMutation({
    mutationFn: async (collectionId: string) => {
      // merge against a fresh read; the list payload may be stale while the dialog was open
      const current = await collectionsApi.get(collectionId)
      const merged = [...new Set([...current.seriesIds, ...seriesIds])]
      await collectionsApi.update(collectionId, { seriesIds: merged })
      return current.name
    },
    onSuccess: (name) => {
      queryClient.invalidateQueries({ queryKey: ['collections'] })
      finish(true, t('addToCollection.added', { name }))
    },
    onError: (e) => finish(false, e instanceof Error ? e.message : t('addToCollection.addFailed')),
  })

  const createMutation = useMutation({
    mutationFn: () => collectionsApi.create({ name: newName.trim(), ordered: false, seriesIds }),
    onSuccess: (created) => {
      queryClient.invalidateQueries({ queryKey: ['collections'] })
      finish(true, t('addToCollection.created', { name: created.name }))
    },
    onError: (e) => finish(false, e instanceof Error ? e.message : t('addToCollection.createFailed')),
  })

  const collections = useMemo(() => listQuery.data?.content ?? [], [listQuery.data])
  const visible = useMemo(() => {
    const f = filter.trim().toLowerCase()
    return f ? collections.filter((c) => c.name.toLowerCase().includes(f)) : collections
  }, [collections, filter])
  const pending = addMutation.isPending || createMutation.isPending
  // with no existing collections, creating is the only possible action
  const creatingOpen = creating || (listQuery.isSuccess && collections.length === 0)

  return (
    <Dialog open={open} onOpenChange={onOpenChange} title={t('addToCollection.title', { count: seriesIds.length })} size="sm">
      <div className="px-5 pt-4">
        <input
          type="search"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder={t('addToCollection.filterPlaceholder')}
          className="h-9 w-full rounded-lg border border-line bg-surface px-3 text-base text-ink transition-colors placeholder:text-ink-3 focus:border-accent/70 focus:outline-none"
        />
      </div>
      <div className="max-h-64 overflow-y-auto py-2">
        {listQuery.isPending ? (
          <div className="space-y-3 px-5 py-2">
            <Skeleton className="h-8 w-full" />
            <Skeleton className="h-8 w-full" />
            <Skeleton className="h-8 w-2/3" />
          </div>
        ) : listQuery.isLoadingError ? (
          <p className="px-5 py-3 text-sm text-ink-3">
            {t('collections.loadError')}{' '}
            <button type="button" onClick={() => listQuery.refetch()} className="cursor-pointer text-accent-strong hover:underline">
              {t('common:action.retry')}
            </button>
          </p>
        ) : visible.length === 0 ? (
          <p className="px-5 py-3 text-sm text-ink-3">
            {collections.length === 0 ? t('collections.empty') : t('addToCollection.noMatch')}
          </p>
        ) : (
          visible.map((c) => (
            <button
              key={c.id}
              type="button"
              disabled={pending}
              onClick={() => addMutation.mutate(c.id)}
              className="flex w-full cursor-pointer items-center justify-between gap-3 px-5 py-2 text-left transition-colors hover:bg-raised disabled:pointer-events-none disabled:opacity-50"
            >
              <span className="min-w-0">
                <span className="block truncate text-sm text-ink">{c.name}</span>
                <span className="block text-xs text-ink-3">{t('seriesCount', { count: c.seriesIds.length })}</span>
              </span>
              <Plus className="size-4 shrink-0 text-ink-3" />
            </button>
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
              label={t('addToCollection.newLabel')}
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              placeholder={t('namePlaceholder')}
              className="flex-1"
              autoFocus
            />
            <Button type="submit" variant="primary" loading={createMutation.isPending} disabled={!newName.trim() || pending}>
              {t('common:action.create')}
            </Button>
            {collections.length > 0 && (
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
            {t('addToCollection.newLabel')}
          </button>
        )}
      </div>
    </Dialog>
  )
}
