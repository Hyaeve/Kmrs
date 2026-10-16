import { useTranslation } from 'react-i18next'
import { useDocumentTitle } from '@/lib/hooks/useDocumentTitle'
import { HistoryBackButton } from '@/components/ui/BackButton'
import { PageHeader } from '@/components/ui/PageHeader'
import { SeriesGrid } from '@/components/browse/SeriesGrid'

export function BrowseSeriesPage() {
  const { t } = useTranslation('browse')

  useDocumentTitle(t('series.all'))

  return (
    <div>
      <HistoryBackButton to="/dashboard" className="mb-2 -ml-2" />
      <PageHeader title={t('series.all')} />
      <SeriesGrid />
    </div>
  )
}
