import { useTranslation } from 'react-i18next'
import { useDocumentTitle } from '@/lib/hooks/useDocumentTitle'
import { HistoryBackButton } from '@/components/ui/BackButton'
import { PageHeader } from '@/components/ui/PageHeader'
import { ReadListsGrid } from '@/components/browse/ReadListsGrid'

export function BrowseReadListsPage() {
  const { t } = useTranslation('browse')

  useDocumentTitle(t('readlists.title'))

  return (
    <div>
      <HistoryBackButton to="/dashboard" className="mb-2 -ml-2" />
      <PageHeader title={t('readlists.title')} />
      <ReadListsGrid searchable />
    </div>
  )
}
