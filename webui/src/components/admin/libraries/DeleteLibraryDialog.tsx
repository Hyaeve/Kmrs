import { useMutation, useQueryClient } from '@tanstack/react-query'
import { Trans, useTranslation } from 'react-i18next'
import { librariesApi } from '@/lib/api/libraries'
import type { LibraryDto } from '@/lib/api/types'
import { Button } from '@/components/ui/Button'
import { Dialog } from '@/components/ui/Dialog'

interface DeleteLibraryDialogProps {
  library: LibraryDto | null
  onOpenChange: (open: boolean) => void
}

export function DeleteLibraryDialog({ library, onOpenChange }: DeleteLibraryDialogProps) {
  const { t } = useTranslation('admin-maintenance')
  const queryClient = useQueryClient()
  const del = useMutation({
    mutationFn: (id: string) => librariesApi.delete(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['libraries'] })
      onOpenChange(false)
    },
  })

  return (
    <Dialog open={!!library} onOpenChange={onOpenChange} title={t('libraries.deleteTitle')} size="sm">
      <div className="px-5 py-4">
        <p className="text-sm text-ink-2">
          <Trans
            i18nKey="libraries.deleteBody"
            ns="admin-maintenance"
            values={{ name: library?.name }}
            components={{ name: <span className="font-medium text-ink" /> }}
          />
        </p>
        {del.isError && (
          <p className="mt-3 text-[13px] text-danger">
            {del.error instanceof Error ? del.error.message : t('libraries.deleteFailed')}
          </p>
        )}
      </div>
      <div className="flex justify-end gap-2 border-t border-line px-5 py-3.5">
        <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
          {t('common:action.cancel')}
        </Button>
        <Button type="button" variant="danger" loading={del.isPending} onClick={() => library && del.mutate(library.id)}>
          {t('common:action.delete')}
        </Button>
      </div>
    </Dialog>
  )
}
