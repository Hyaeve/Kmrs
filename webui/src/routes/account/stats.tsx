import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useDocumentTitle } from '@/lib/hooks/useDocumentTitle'
import { PageHeader } from '@/components/ui/PageHeader'
import { LibraryFilter } from '@/components/account/stats/LibraryFilter'
import { SummaryCards } from '@/components/account/stats/SummaryCards'
import { HeatmapSection } from '@/components/account/stats/HeatmapSection'
import { StatusSection } from '@/components/account/stats/StatusSection'
import { ActivityCharts } from '@/components/account/stats/ActivityCharts'
import { TopsSection } from '@/components/account/stats/TopsSection'

export function AccountStatsPage() {
  const { t } = useTranslation('stats')
  useDocumentTitle(t('title'))
  const [libraryId, setLibraryId] = useState<string>()

  return (
    <div className="max-w-3xl">
      <PageHeader
        title={t('title')}
        subtitle={t('subtitle')}
        actions={<LibraryFilter value={libraryId} onChange={setLibraryId} />}
      />
      <div className="space-y-6">
        <SummaryCards libraryId={libraryId} />
        <HeatmapSection libraryId={libraryId} />
        <StatusSection libraryId={libraryId} />
        <ActivityCharts libraryId={libraryId} />
        <TopsSection libraryId={libraryId} />
      </div>
    </div>
  )
}
