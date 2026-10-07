import { useTranslation } from 'react-i18next'
import type { Role } from '@/lib/api/types'
import { useAuthStore } from '@/lib/store/auth'
import { Chip } from '@/components/ui/Chip'
import { Section } from './Section'

const roleKeys: Record<Role, string> = {
  ADMIN: 'admin',
  USER: 'user',
  FILE_DOWNLOAD: 'fileDownload',
  PAGE_STREAMING: 'pageStreaming',
  KOBO_SYNC: 'koboSync',
  KOREADER_SYNC: 'koreaderSync',
}

export function ProfileSection() {
  const { t } = useTranslation('account')
  const user = useAuthStore((s) => s.user)
  if (!user) return null
  return (
    <Section title={t('profile.title')}>
      <dl className="grid grid-cols-[110px_1fr] gap-x-4 gap-y-3 text-sm sm:grid-cols-[160px_1fr]">
        <dt className="text-ink-3">{t('profile.email')}</dt>
        <dd className="min-w-0 truncate text-ink">{user.email}</dd>
        <dt className="text-ink-3">{t('profile.roles')}</dt>
        <dd className="flex min-w-0 flex-wrap gap-1.5">
          {user.roles.map((r) => (
            <Chip key={r}>{t(`profile.role.${roleKeys[r]}`)}</Chip>
          ))}
        </dd>
        <dt className="text-ink-3">{t('profile.libraries')}</dt>
        <dd className="text-ink">
          {user.sharedAllLibraries
            ? t('profile.allLibraries')
            : t('profile.sharedLibraries', { count: user.sharedLibrariesIds.length })}
        </dd>
      </dl>
    </Section>
  )
}
