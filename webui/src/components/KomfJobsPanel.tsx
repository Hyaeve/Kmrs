import { useEffect } from 'react'
import { AnimatePresence, motion } from 'motion/react'
import { CircleNotch, X } from '@phosphor-icons/react'
import { useTranslation } from 'react-i18next'
import { useKomfIntegration } from '@/lib/hooks/useKomfIntegration'
import { startKomfFirehose, stopKomfFirehose, useKomfJobs, type TrackedJob } from '@/lib/store/komfJobs'
import { IconButton } from '@/components/ui/IconButton'

/** Beyond this many non-failed jobs the panel collapses them into a summary card. */
const COLLAPSE_AT = 3
const MAX_FAILED_CARDS = 3

export function KomfJobsPanel() {
  const { t } = useTranslation()
  const jobs = useKomfJobs((s) => s.jobs)
  const dismiss = useKomfJobs((s) => s.dismiss)
  const komfReady = useKomfIntegration()
  useEffect(() => {
    if (komfReady) startKomfFirehose()
    else stopKomfFirehose()
  }, [komfReady])
  const list = Object.values(jobs)
  if (list.length === 0) return null

  const failed = list.filter((j) => j.done && j.failed)
  const rest = list.filter((j) => !j.done || !j.failed)
  const collapsed = rest.length > COLLAPSE_AT
  const active = rest.filter((j) => !j.done)
  const shown = collapsed ? failed.slice(0, MAX_FAILED_CARDS) : list
  const summaryText = active[0]?.text ?? rest[rest.length - 1]?.text

  const card = (job: TrackedJob) => (
    <motion.div
      key={job.id}
      initial={{ opacity: 0, y: 24 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: 24 }}
      transition={{ duration: 0.2, ease: [0.16, 1, 0.3, 1] }}
      className="pointer-events-auto flex min-w-72 items-center gap-3 rounded-xl border border-line bg-overlay px-4 py-3 shadow-pop"
    >
      {!job.done && <CircleNotch className="size-4 shrink-0 animate-spin text-ink-3" />}
      <div className="min-w-0 flex-1">
        <div className="truncate text-sm font-medium text-ink">{job.label}</div>
        <div className={job.failed ? 'truncate text-xs text-danger' : 'truncate text-xs text-ink-2'}>
          {job.failed ?? job.text}
        </div>
      </div>
      {job.failed && (
        <IconButton label={t('action.dismiss')} className="-mr-1 size-7" onClick={() => dismiss(job.id)}>
          <X className="size-4" />
        </IconButton>
      )}
    </motion.div>
  )

  return (
    <div className="pointer-events-none fixed right-6 bottom-[max(1.5rem,env(safe-area-inset-bottom))] z-20 flex flex-col items-end gap-2">
      <AnimatePresence>
        {shown.map(card)}
        {collapsed && (
          <motion.div
            key="summary"
            initial={{ opacity: 0, y: 24 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 24 }}
            transition={{ duration: 0.2, ease: [0.16, 1, 0.3, 1] }}
            className="pointer-events-auto flex min-w-72 items-center gap-3 rounded-xl border border-line bg-overlay px-4 py-3 shadow-pop"
          >
            {active.length > 0 && <CircleNotch className="size-4 shrink-0 animate-spin text-ink-3" />}
            <div className="min-w-0 flex-1">
              <div className="truncate text-sm font-medium text-ink">{t('komf.jobsSummary', { count: rest.length })}</div>
              <div className="truncate text-xs text-ink-2">{summaryText}</div>
              {failed.length > MAX_FAILED_CARDS && (
                <div className="truncate text-xs text-danger">
                  {t('komf.moreFailed', { count: failed.length - MAX_FAILED_CARDS })}
                </div>
              )}
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  )
}
