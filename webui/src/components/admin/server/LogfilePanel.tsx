import { useTranslation } from 'react-i18next'
import { FileArrowDown } from '@phosphor-icons/react'
import { serverApi } from '@/lib/api/server'
import { Section } from '@/components/account/Section'

export function LogfilePanel() {
  const { t } = useTranslation('admin-settings')
  return (
    <Section title={t('logfile.title')}>
      <div className="flex flex-wrap items-center justify-between gap-x-6 gap-y-2.5">
        <p className="text-sm text-ink-3">{t('logfile.helper')}</p>
        <a
          href={serverApi.logfileUrl()}
          download
          className="inline-flex h-8 shrink-0 cursor-pointer items-center justify-center gap-1.5 rounded-lg border border-line bg-raised px-3 text-[13px] whitespace-nowrap text-ink transition-all duration-150 hover:border-line-strong hover:bg-overlay active:scale-[0.98]"
        >
          <FileArrowDown className="size-4" />
          {t('common:action.download')}
        </a>
      </div>
    </Section>
  )
}
