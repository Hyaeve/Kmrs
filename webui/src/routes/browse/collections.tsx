import { useTranslation } from 'react-i18next'
import { useDocumentTitle } from '@/lib/hooks/useDocumentTitle'
import { HistoryBackButton } from '@/components/ui/BackButton'
import { PageHeader } from '@/components/ui/PageHeader'
import { CollectionsGrid } from '@/components/browse/CollectionsGrid'

export function BrowseCollectionsPage() {
  const { t } = useTranslation('browse')

  useDocumentTitle(t('collections.title'))

  return (
    <div>
      <HistoryBackButton to="/dashboard" className="mb-2 -ml-2" />
      <PageHeader title={t('collections.title')} />
      <CollectionsGrid searchable />
    </div>
  )
}
