import { useTranslation } from 'react-i18next'
import { useDocumentTitle } from '@/lib/hooks/useDocumentTitle'
import { PageHeader } from '@/components/ui/PageHeader'
import { ReaderSection } from '@/components/account/ReaderSection'

export function AccountReaderPage() {
  const { t } = useTranslation('account')
  useDocumentTitle(t('reader.title'))

  return (
    <div className="max-w-3xl">
      <PageHeader title={t('reader.title')} subtitle={t('reader.subtitle')} />
      <ReaderSection />
    </div>
  )
}
