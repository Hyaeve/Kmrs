import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { actuatorApi } from '@/lib/api/settings'
import { Button } from '@/components/ui/Button'
import { Chip } from '@/components/ui/Chip'
import { Skeleton } from '@/components/ui/Skeleton'
import { Section } from '@/components/account/Section'
import { formatInterval, targetLabel } from './format'

export function ScheduledTasksPanel() {
  const { t } = useTranslation('admin-settings')
  const query = useQuery({ queryKey: ['admin', 'scheduled-tasks'], queryFn: actuatorApi.scheduledTasks })

  return (
    <Section title={t('scheduled.title')}>
      {query.isLoading && (
        <div className="flex flex-col gap-2.5">
          <Skeleton className="h-7 w-full" />
          <Skeleton className="h-7 w-2/3" />
        </div>
      )}
      {query.isLoadingError && (
        <div className="flex items-center gap-3">
          <p className="text-sm text-danger">
            {query.error instanceof Error ? query.error.message : t('scheduled.loadFailed')}
          </p>
          <Button size="sm" onClick={() => void query.refetch()}>
            {t('common:action.retry')}
          </Button>
        </div>
      )}
      {query.data && (
        <>
          {query.data.fixedRate.length === 0 ? (
            <p className="text-sm text-ink-3">{t('scheduled.empty')}</p>
          ) : (
            <ul className="flex flex-col divide-y divide-line">
              {query.data.fixedRate.map((task) => (
                <li key={task.runnable.target} className="flex items-center justify-between gap-4 py-2.5">
                  <span className="truncate font-mono text-[13px] text-ink-2" title={task.runnable.target}>
                    {targetLabel(task.runnable.target)}
                  </span>
                  <Chip className="shrink-0">{formatInterval(task.interval)}</Chip>
                </li>
              ))}
            </ul>
          )}
        </>
      )}
    </Section>
  )
}
