import { useMutation, useQueryClient } from '@tanstack/react-query'
import { Trans, useTranslation } from 'react-i18next'
import { booksApi } from '@/lib/api/books'
import type { BookDto } from '@/lib/api/types'
import { Button } from '@/components/ui/Button'
import { Dialog } from '@/components/ui/Dialog'

interface DeleteBookFileDialogProps {
  book: BookDto | null
  onOpenChange: (open: boolean) => void
}

export function DeleteBookFileDialog({ book, onOpenChange }: DeleteBookFileDialogProps) {
  const { t } = useTranslation('admin-maintenance')
  const queryClient = useQueryClient()

  const mutation = useMutation({
    mutationFn: (bookId: string) => booksApi.deleteFile(bookId),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['admin', 'duplicates'] })
      void queryClient.invalidateQueries({ queryKey: ['books'] })
      onOpenChange(false)
    },
  })

  const close = () => {
    mutation.reset()
    onOpenChange(false)
  }

  return (
    <Dialog
      open={!!book}
      onOpenChange={(o) => {
        if (!o) close()
      }}
      title={t('detail:menu.deleteFile')}
      size="sm"
    >
      <div className="px-5 py-4">
        <p className="text-sm text-ink-2">
          <Trans
            i18nKey="duplicates.deleteBody"
            ns="admin-maintenance"
            values={{ name: book?.metadata.title || book?.name }}
            components={{ name: <span className="font-medium text-ink" /> }}
          />
        </p>
        {mutation.isError && (
          <p className="mt-3 text-sm text-danger">
            {mutation.error instanceof Error ? mutation.error.message : t('duplicates.deleteFailed')}
          </p>
        )}
      </div>
      <div className="flex justify-end gap-2 border-t border-line px-5 py-3.5">
        <Button variant="ghost" onClick={close}>
          {t('common:action.cancel')}
        </Button>
        <Button variant="danger" loading={mutation.isPending} onClick={() => book && mutation.mutate(book.id)}>
          {t('detail:menu.deleteFile')}
        </Button>
      </div>
    </Dialog>
  )
}
