import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { X } from '@phosphor-icons/react'
import { collectionsApi, readlistsApi } from '@/lib/api/collections'
import type { CollectionDto, ReadListDto } from '@/lib/api/types'
import { showToast } from '@/lib/store/toast'
import { Tooltip } from '@/components/ui/Tooltip'
import { CardMenuButton } from '@/components/media/CardFrame'
import { ConfirmDeleteDialog } from './ConfirmDeleteDialog'

interface MembershipRemoveButtonProps {
  /** tooltip and aria-label on the corner button */
  label: string
  /** group name shown in the confirm dialog */
  name: string
  deleteTitle: string
  /** removing this member empties the group, which deletes it (komga parity) */
  deletesGroup: boolean
  pending: boolean
  onRemove: () => void
}

function MembershipRemoveButton({ label, name, deleteTitle, deletesGroup, pending, onRemove }: MembershipRemoveButtonProps) {
  const [confirmOpen, setConfirmOpen] = useState(false)
  return (
    <>
      <Tooltip content={label}>
        <CardMenuButton
          aria-label={label}
          disabled={pending}
          onClick={() => (deletesGroup ? setConfirmOpen(true) : onRemove())}
        >
          <X className="size-4" weight="bold" />
        </CardMenuButton>
      </Tooltip>
      {deletesGroup && (
        <ConfirmDeleteDialog
          open={confirmOpen}
          onOpenChange={setConfirmOpen}
          title={deleteTitle}
          name={name}
          loading={pending}
          onConfirm={onRemove}
        />
      )}
    </>
  )
}

export function RemoveFromCollectionButton({ collection, seriesId }: { collection: CollectionDto; seriesId: string }) {
  const { t } = useTranslation('detail')
  const queryClient = useQueryClient()
  const remaining = collection.seriesIds.filter((id) => id !== seriesId)
  const lastMember = remaining.length === 0
  const mutation = useMutation({
    mutationFn: () =>
      lastMember ? collectionsApi.delete(collection.id) : collectionsApi.update(collection.id, { seriesIds: remaining }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['collections'] })
      showToast(lastMember ? t('toast.collectionDeleted') : t('toast.removedFromCollection', { name: collection.name }))
    },
    onError: (e) => showToast(e instanceof Error ? e.message : t('toast.removeFromCollectionFailed')),
  })
  return (
    <MembershipRemoveButton
      label={t('removeFromCollection', { name: collection.name })}
      name={collection.name}
      deleteTitle={t('collection.deleteTitle')}
      deletesGroup={lastMember}
      pending={mutation.isPending}
      onRemove={() => mutation.mutate()}
    />
  )
}

export function RemoveFromReadListButton({ readlist, bookId }: { readlist: ReadListDto; bookId: string }) {
  const { t } = useTranslation('detail')
  const queryClient = useQueryClient()
  const remaining = readlist.bookIds.filter((id) => id !== bookId)
  const lastMember = remaining.length === 0
  const mutation = useMutation({
    mutationFn: () => (lastMember ? readlistsApi.delete(readlist.id) : readlistsApi.update(readlist.id, { bookIds: remaining })),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['readlists'] })
      showToast(lastMember ? t('toast.readListDeleted') : t('toast.removedFromReadList', { name: readlist.name }))
    },
    onError: (e) => showToast(e instanceof Error ? e.message : t('toast.removeFromReadListFailed')),
  })
  return (
    <MembershipRemoveButton
      label={t('removeFromReadList', { name: readlist.name })}
      name={readlist.name}
      deleteTitle={t('readList.deleteTitle')}
      deletesGroup={lastMember}
      pending={mutation.isPending}
      onRemove={() => mutation.mutate()}
    />
  )
}
