import { useTranslation } from 'react-i18next'
import { useAuthStore } from '@/lib/store/auth'
import { useDocumentTitle } from '@/lib/hooks/useDocumentTitle'
import { PageHeader } from '@/components/ui/PageHeader'
import { ProfileSection } from '@/components/account/ProfileSection'
import { LanguageSection } from '@/components/account/LanguageSection'

export function AccountProfilePage() {
  const { t } = useTranslation('account')
  const user = useAuthStore((s) => s.user)
  useDocumentTitle(t('profile.title'))

  return (
    <div className="max-w-3xl">
      <PageHeader title={t('profile.title')} subtitle={user?.email} />
      <div className="space-y-6">
        <ProfileSection />
        <LanguageSection />
      </div>
    </div>
  )
}
