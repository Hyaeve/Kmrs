import { useTranslation } from 'react-i18next'
import { LanguageSwitcher } from '@/components/LanguageSwitcher'
import { Section, SettingRow } from './Section'

export function LanguageSection() {
  const { t } = useTranslation('account')
  return (
    <Section title={t('language.title')}>
      <SettingRow label={t('language.label')}>
        <LanguageSwitcher />
      </SettingRow>
    </Section>
  )
}
