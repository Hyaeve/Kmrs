import { useTranslation } from 'react-i18next'
import { useDocumentTitle } from '@/lib/hooks/useDocumentTitle'
import { PageHeader } from '@/components/ui/PageHeader'
import { ActivitySection } from '@/components/account/ActivitySection'
import { PasswordSection } from '@/components/account/PasswordSection'

export function AccountSecurityPage() {
  const { t } = useTranslation('account')
  useDocumentTitle(t('security.title'))

  return (
    <div className="max-w-3xl">
      <PageHeader title={t('security.title')} subtitle={t('security.subtitle')} />
      <div className="space-y-6">
        <PasswordSection />
        <ActivitySection />
      </div>
    </div>
  )
}
