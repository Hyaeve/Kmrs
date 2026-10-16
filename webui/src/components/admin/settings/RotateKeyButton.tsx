import { useState } from 'react'
import { useMutation } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Key, WarningCircle } from '@phosphor-icons/react'
import { settingsApi } from '@/lib/api/settings'
import { Button } from '@/components/ui/Button'
import { Dialog } from '@/components/ui/Dialog'

export function RotateKeyButton() {
  const { t } = useTranslation('admin-settings')
  const [open, setOpen] = useState(false)
  const [done, setDone] = useState(false)

  const mutation = useMutation({
    mutationFn: () => settingsApi.update({ renewRememberMeKey: true }),
    onSuccess: () => {
      setOpen(false)
      setDone(true)
    },
  })

  return (
    <>
      <div className="flex flex-wrap items-center justify-end gap-3">
        {done && <span className="text-xs text-accent-strong">{t('rotateKey.done')}</span>}
        <Button
          size="sm"
          onClick={() => {
            setDone(false)
            setOpen(true)
          }}
        >
          <Key className="size-4" />
          {t('rotateKey.button')}
        </Button>
      </div>
      <Dialog open={open} onOpenChange={setOpen} title={t('rotateKey.button')} size="sm">
        <div className="flex flex-col gap-4 p-5">
          <p className="flex items-start gap-2 text-sm text-ink-2">
            <WarningCircle className="mt-0.5 size-4 shrink-0 text-danger" />
            {t('rotateKey.body')}
          </p>
          {mutation.isError && (
            <p className="text-sm text-danger">
              {mutation.error instanceof Error ? mutation.error.message : t('rotateKey.failed')}
            </p>
          )}
          <div className="flex justify-end gap-2">
            <Button onClick={() => setOpen(false)}>{t('common:action.cancel')}</Button>
            <Button variant="danger" loading={mutation.isPending} onClick={() => mutation.mutate()}>
              {t('rotateKey.confirm')}
            </Button>
          </div>
        </div>
      </Dialog>
    </>
  )
}
