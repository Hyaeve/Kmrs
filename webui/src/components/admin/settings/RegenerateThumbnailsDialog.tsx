import { useMutation } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { booksApi } from '@/lib/api/books'
import { Button } from '@/components/ui/Button'
import { Dialog } from '@/components/ui/Dialog'

interface RegenerateThumbnailsDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function RegenerateThumbnailsDialog({ open, onOpenChange }: RegenerateThumbnailsDialogProps) {
  const { t } = useTranslation('admin-settings')
  const mutation = useMutation({
    mutationFn: (forBiggerResultOnly: boolean) => booksApi.regenerateThumbnails(forBiggerResultOnly),
    onSuccess: () => onOpenChange(false),
  })

  // a failed attempt must not greet the user on the next open
  const handleOpenChange = (open: boolean) => {
    if (!open) mutation.reset()
    onOpenChange(open)
  }

  return (
    <Dialog open={open} onOpenChange={handleOpenChange} title={t('regenerate.title')} size="sm">
      <div className="flex flex-col gap-4 p-5">
        <p className="text-sm text-ink-2">{t('regenerate.body')}</p>
        {mutation.isError && (
          <p className="text-sm text-danger">
            {mutation.error instanceof Error ? mutation.error.message : t('regenerate.failed')}
          </p>
        )}
        <div className="flex flex-wrap justify-end gap-2">
          <Button variant="ghost" onClick={() => handleOpenChange(false)}>
            {t('regenerate.skip')}
          </Button>
          <Button
            disabled={mutation.isPending}
            loading={mutation.isPending && mutation.variables === false}
            onClick={() => mutation.mutate(false)}
          >
            {t('regenerate.all')}
          </Button>
          <Button
            variant="primary"
            disabled={mutation.isPending}
            loading={mutation.isPending && mutation.variables === true}
            onClick={() => mutation.mutate(true)}
          >
            {t('regenerate.biggerOnly')}
          </Button>
        </div>
      </div>
    </Dialog>
  )
}
