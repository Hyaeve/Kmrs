import { useTranslation } from 'react-i18next'
import { formatBytes } from '@/lib/utils/format'
import { Section } from '@/components/account/Section'
import { useServerStats } from '../stats'

interface ContentStat {
  labelKey: string
  value: number | undefined
  format?: (value: number) => string
}

export function ContentCounts() {
  const { t } = useTranslation('admin-settings')
  const server = useServerStats()
  const totals = server.data?.totals

  const stats: ContentStat[] = [
    { labelKey: 'content.libraries', value: totals?.libraries },
    { labelKey: 'content.series', value: totals?.series },
    { labelKey: 'content.books', value: totals?.books },
    { labelKey: 'content.collections', value: totals?.collections },
    { labelKey: 'content.readLists', value: totals?.readlists },
    { labelKey: 'content.sidecars', value: totals?.sidecars },
    { labelKey: 'content.totalSize', value: totals?.fileSize, format: formatBytes },
  ]

  return (
    <Section title={t('sections.content')}>
      <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 xl:grid-cols-6">
        {stats.map((s) => (
          <div key={s.labelKey}>
            <p className="text-2xl font-semibold text-ink">
              {server.isLoading ? '…' : s.value === undefined ? '—' : (s.format?.(s.value) ?? s.value)}
            </p>
            <p className="mt-0.5 text-xs text-ink-3">{t(s.labelKey)}</p>
          </div>
        ))}
      </div>
    </Section>
  )
}
