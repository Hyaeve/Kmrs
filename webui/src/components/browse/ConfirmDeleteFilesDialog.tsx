import { Trans, useTranslation } from 'react-i18next'
import { Dialog } from '@/components/ui/Dialog'
import { Button } from '@/components/ui/Button'

interface ConfirmDeleteFilesDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  count: number
  /** selects the entity noun in the confirmation sentence */
  kind: 'book' | 'series'
  loading: boolean
  onConfirm: () => void
}

export function ConfirmDeleteFilesDialog({ open, onOpenChange, count, kind, loading, onConfirm }: ConfirmDeleteFilesDialogProps) {
  const { t } = useTranslation('browse')
  return (
    <Dialog open={open} onOpenChange={onOpenChange} title={t('selection.deleteFiles')} size="sm">
      <div className="px-5 py-4">
        <p className="text-sm text-ink-2">
          <Trans
            i18nKey="browse:deleteConfirm.body"
            context={kind}
            count={count}
            components={{ num: <span className="font-medium text-ink" /> }}
          />
        </p>
      </div>
      <div className="flex justify-end gap-2 border-t border-line px-5 py-3.5">
        <Button variant="ghost" onClick={() => onOpenChange(false)}>
          {t('common:action.cancel')}
        </Button>
        <Button variant="danger" loading={loading} onClick={onConfirm}>
          {t('selection.deleteFiles')}
        </Button>
      </div>
    </Dialog>
  )
}
