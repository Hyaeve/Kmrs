import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { serverApi } from '@/lib/api/users'
import { useDocumentTitle } from '@/lib/hooks/useDocumentTitle'
import { PageHeader } from '@/components/ui/PageHeader'
import { StatsGrid } from '@/components/admin/server/StatsGrid'
import { ContentCounts } from '@/components/admin/server/ContentCounts'
import { TaskQueuePanel } from '@/components/admin/server/TaskQueuePanel'
import { TaskExecutionPanel } from '@/components/admin/server/TaskExecutionPanel'
import { ScheduledTasksPanel } from '@/components/admin/server/ScheduledTasksPanel'
import { SessionsPanel } from '@/components/admin/server/SessionsPanel'
import { DangerZone } from '@/components/admin/server/DangerZone'

export function AdminServerPage() {
  const { t } = useTranslation('admin-settings')
  const { data: info } = useQuery({ queryKey: ['server-info'], queryFn: serverApi.info, staleTime: Infinity })

  useDocumentTitle(t('server.title'))

  const version = info?.build?.version
  const commit = info?.git?.commit?.id
  const subtitle = version ? (commit ? t('server.subtitleWithCommit', { version, commit }) : t('server.subtitle', { version })) : undefined

  return (
    <div>
      <PageHeader title={t('server.title')} subtitle={subtitle} />
      <div className="space-y-6">
        <StatsGrid />
        <ContentCounts />
        <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
          <TaskQueuePanel />
          <TaskExecutionPanel />
        </div>
        <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
          <ScheduledTasksPanel />
          <SessionsPanel />
        </div>
        <DangerZone />
      </div>
    </div>
  )
}
