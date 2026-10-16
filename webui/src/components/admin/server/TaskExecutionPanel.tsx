import { useTranslation } from 'react-i18next'
import { Skeleton } from '@/components/ui/Skeleton'
import { Section } from '@/components/account/Section'
import { formatDuration, taskTypeLabel } from './format'
import { useServerStats } from '../stats'

export function TaskExecutionPanel() {
  const { t } = useTranslation('admin-settings')
  const stats = useServerStats()

  const types = stats.data?.tasks.types.filter((task) => task.executions > 0) ?? []
  const failureCount = stats.data?.tasks.types.reduce((sum, task) => sum + task.failures, 0) ?? 0

  return (
    <Section title={t('taskExecution.title')}>
      {stats.isLoading && (
        <div className="flex flex-col gap-2.5">
          <Skeleton className="h-7 w-full" />
          <Skeleton className="h-7 w-2/3" />
        </div>
      )}
      {!stats.isLoading && types.length === 0 && <p className="text-sm text-ink-3">{t('taskExecution.empty')}</p>}
      {types.length > 0 && (
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="text-left text-xs text-ink-3">
                <th className="pb-2 pr-4 font-medium">{t('taskExecution.type')}</th>
                <th className="pb-2 pr-4 text-right font-medium">{t('taskExecution.runs')}</th>
                <th className="pb-2 pr-4 text-right font-medium">{t('taskExecution.totalTime')}</th>
                <th className="pb-2 text-right font-medium">{t('taskExecution.longest')}</th>
              </tr>
            </thead>
            <tbody>
              {types.map((task) => (
                <tr key={task.type} className="border-t border-line">
                  <td className="py-2 pr-4 whitespace-nowrap text-ink-2">{taskTypeLabel(task.type)}</td>
                  <td className="py-2 pr-4 text-right font-mono text-xs text-ink">{task.executions}</td>
                  <td className="py-2 pr-4 text-right whitespace-nowrap text-ink-2">{formatDuration(task.totalTimeMs / 1000)}</td>
                  <td className="py-2 text-right whitespace-nowrap text-ink-2">{formatDuration(task.maxTimeMs / 1000)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {failureCount > 0 && <p className="mt-3 text-xs text-danger">{t('taskExecution.failures', { count: failureCount })}</p>}
    </Section>
  )
}
