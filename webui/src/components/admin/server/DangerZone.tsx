import { useState } from 'react'
import { useMutation } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { FileArrowDown, Power, WarningCircle } from '@phosphor-icons/react'
import { actuatorApi } from '@/lib/api/settings'
import { Button } from '@/components/ui/Button'
import { Dialog } from '@/components/ui/Dialog'

export function DangerZone() {
  const { t } = useTranslation('admin-settings')
  const [confirmOpen, setConfirmOpen] = useState(false)
  const [shuttingDown, setShuttingDown] = useState(false)

  const shutdown = useMutation({
    mutationFn: actuatorApi.shutdown,
    onSuccess: () => {
      setConfirmOpen(false)
      setShuttingDown(true)
    },
  })

  if (shuttingDown)
    return (
      <section className="rounded-xl border border-danger/40 bg-danger/5 p-5">
        <h2 className="text-[15px] font-semibold text-danger">{t('danger.shuttingDown')}</h2>
        <p className="mt-2 text-sm text-ink-2">{t('danger.shuttingDownBody')}</p>
      </section>
    )

  return (
    <section className="rounded-xl border border-danger/40 bg-surface p-5">
      <h2 className="mb-4 text-[15px] font-semibold text-danger">{t('danger.title')}</h2>
      <div className="flex flex-wrap items-center justify-between gap-x-6 gap-y-2.5 py-3 first:pt-0">
        <div className="min-w-0 max-w-md">
          <p className="text-sm text-ink-2">{t('danger.logfile')}</p>
          <p className="mt-0.5 text-xs text-ink-3">{t('danger.logfileHelper')}</p>
        </div>
        <a
          href={actuatorApi.logfileUrl()}
          download
          className="inline-flex h-8 shrink-0 cursor-pointer items-center justify-center gap-1.5 rounded-lg border border-line bg-raised px-3 text-[13px] whitespace-nowrap text-ink transition-all duration-150 hover:border-line-strong hover:bg-overlay active:scale-[0.98]"
        >
          <FileArrowDown className="size-4" />
          {t('common:action.download')}
        </a>
      </div>
      <div className="flex flex-wrap items-center justify-between gap-x-6 gap-y-2.5 py-3 last:pb-0">
        <div className="min-w-0 max-w-md">
          <p className="text-sm text-ink-2">{t('danger.shutdown')}</p>
          <p className="mt-0.5 text-xs text-ink-3">{t('danger.shutdownHelper')}</p>
        </div>
        <Button size="sm" variant="danger" onClick={() => setConfirmOpen(true)}>
          <Power className="size-4" />
          {t('danger.shutdownButton')}
        </Button>
      </div>

      <Dialog open={confirmOpen} onOpenChange={setConfirmOpen} title={t('danger.shutdown')} size="sm">
        <div className="flex flex-col gap-4 p-5">
          <p className="flex items-start gap-2 text-sm text-ink-2">
            <WarningCircle className="mt-0.5 size-4 shrink-0 text-danger" />
            {t('danger.shutdownBody')}
          </p>
          {shutdown.isError && (
            <p className="text-sm text-danger">
              {shutdown.error instanceof Error ? shutdown.error.message : t('danger.shutdownFailed')}
            </p>
          )}
          <div className="flex justify-end gap-2">
            <Button onClick={() => setConfirmOpen(false)}>{t('common:action.cancel')}</Button>
            <Button variant="danger" loading={shutdown.isPending} onClick={() => shutdown.mutate()}>
              {t('danger.shutdownButton')}
            </Button>
          </div>
        </div>
      </Dialog>
    </section>
  )
}
