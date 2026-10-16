import { Trans, useTranslation } from 'react-i18next'
import { Dialog } from '@/components/ui/Dialog'
import { Button } from '@/components/ui/Button'

interface ConfirmDeleteDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  title: string
  /** entity name shown in the confirmation copy */
  name: string
  loading: boolean
  onConfirm: () => void
}

export function ConfirmDeleteDialog({ open, onOpenChange, title, name, loading, onConfirm }: ConfirmDeleteDialogProps) {
  const { t } = useTranslation('detail')
  return (
    <Dialog open={open} onOpenChange={onOpenChange} title={title} size="sm">
      <div className="px-5 py-4">
        <p className="text-sm text-ink-2">
          <Trans i18nKey="confirmDelete.body" ns="detail" values={{ name }} components={{ name: <span className="font-medium text-ink" /> }} />
        </p>
      </div>
      <div className="flex justify-end gap-2 border-t border-line px-5 py-3.5">
        <Button variant="ghost" onClick={() => onOpenChange(false)}>
          {t('common:action.cancel')}
        </Button>
        <Button variant="danger" loading={loading} onClick={onConfirm}>
          {t('common:action.delete')}
        </Button>
      </div>
    </Dialog>
  )
}
