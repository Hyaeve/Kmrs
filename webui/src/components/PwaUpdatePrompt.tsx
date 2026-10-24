import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/Button'
import { usePwaStore } from '@/lib/store/pwa'

export function PwaUpdatePrompt() {
  const { t } = useTranslation('pwa')
  const updateServiceWorker = usePwaStore((s) => s.updateServiceWorker)
  const dismissUpdate = usePwaStore((s) => s.dismissUpdate)

  if (!updateServiceWorker) return null

  return (
    <div className="pointer-events-none fixed inset-x-0 bottom-[max(1rem,env(safe-area-inset-bottom))] z-50 flex justify-center px-4">
      <div
        role="region"
        aria-label={t('updateLabel')}
        className="pointer-events-auto flex w-full max-w-md flex-wrap items-center gap-3 rounded-xl border border-line bg-overlay px-4 py-3 shadow-pop"
      >
        <span role="status" className="min-w-32 flex-1 text-sm text-ink">
          {t('updateAvailable')}
        </span>
        <div className="ml-auto flex items-center gap-2">
          <Button variant="primary" size="sm" onClick={() => void updateServiceWorker(true)}>
            {t('reload')}
          </Button>
          <Button size="sm" onClick={dismissUpdate}>
            {t('later')}
          </Button>
        </div>
      </div>
    </div>
  )
}
