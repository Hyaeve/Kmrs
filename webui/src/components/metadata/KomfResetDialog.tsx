import { Trans, useTranslation } from 'react-i18next'
import { Dialog } from '@/components/ui/Dialog'
import { Button } from '@/components/ui/Button'

interface KomfResetDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** series or library name shown in the confirmation copy */
  name: string
  /** metadata-namespace key for the body copy; defaults to the series-level one */
  bodyKey?: string
  loading: boolean
  onConfirm: () => void
}

export function KomfResetDialog({ open, onOpenChange, name, bodyKey = 'reset.body', loading, onConfirm }: KomfResetDialogProps) {
  const { t } = useTranslation('metadata')
  return (
    <Dialog open={open} onOpenChange={onOpenChange} title={t('detail:menu.resetKomf')} size="sm">
      <div className="px-5 py-4">
        <p className="text-sm text-ink-2">
          <Trans i18nKey={bodyKey} ns="metadata" values={{ name }} components={{ name: <span className="font-medium text-ink" /> }} />
        </p>
      </div>
      <div className="flex justify-end gap-2 border-t border-line px-5 py-3.5">
        <Button variant="ghost" onClick={() => onOpenChange(false)}>
          {t('common:action.cancel')}
        </Button>
        <Button variant="danger" loading={loading} onClick={onConfirm}>
          {t('reset.confirm')}
        </Button>
      </div>
    </Dialog>
  )
}
