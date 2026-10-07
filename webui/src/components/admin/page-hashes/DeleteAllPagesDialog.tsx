import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { pageHashesApi } from '@/lib/api/pageHashes'
import type { PageHashKnownDto } from '@/lib/api/types'
import { Button } from '@/components/ui/Button'
import { Dialog } from '@/components/ui/Dialog'

interface DeleteAllPagesDialogProps {
  known: PageHashKnownDto | null
  onOpenChange: (open: boolean) => void
}

export function DeleteAllPagesDialog({ known, onOpenChange }: DeleteAllPagesDialogProps) {
  const { t } = useTranslation('admin-maintenance')
  const queryClient = useQueryClient()

  const mutation = useMutation({
    mutationFn: (hash: string) => pageHashesApi.deleteAll(hash),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['admin', 'page-hashes'] })
      onOpenChange(false)
    },
  })

  const close = () => {
    mutation.reset()
    onOpenChange(false)
  }

  return (
    <Dialog
      open={!!known}
      onOpenChange={(o) => {
        if (!o) close()
      }}
      title={t('pageHashes.deleteAllMatching')}
      size="sm"
    >
      <div className="px-5 py-4">
        <p className="text-sm text-ink-2">
          {t('pageHashes.deleteAllBody', { count: known?.matchCount ?? 0 })}
        </p>
        {mutation.isError && (
          <p className="mt-3 text-sm text-danger">
            {mutation.error instanceof Error ? mutation.error.message : t('pageHashes.deleteAllFailed')}
          </p>
        )}
      </div>
      <div className="flex justify-end gap-2 border-t border-line px-5 py-3.5">
        <Button variant="ghost" onClick={close}>
          {t('common:action.cancel')}
        </Button>
        <Button variant="danger" loading={mutation.isPending} onClick={() => known && mutation.mutate(known.hash)}>
          {t('pageHashes.deleteAll')}
        </Button>
      </div>
    </Dialog>
  )
}
