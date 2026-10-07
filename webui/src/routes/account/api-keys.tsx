import { useTranslation } from 'react-i18next'
import { useDocumentTitle } from '@/lib/hooks/useDocumentTitle'
import { PageHeader } from '@/components/ui/PageHeader'
import { ApiKeysSection } from '@/components/account/ApiKeysSection'

export function AccountApiKeysPage() {
  const { t } = useTranslation('account')
  useDocumentTitle(t('apiKeys.title'))

  return (
    <div className="max-w-3xl">
      <PageHeader title={t('apiKeys.title')} subtitle={t('apiKeys.subtitle')} />
      <ApiKeysSection />
    </div>
  )
}
