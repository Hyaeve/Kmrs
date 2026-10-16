import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { statsApi } from '@/lib/api/stats'
import type { NamedValueDto } from '@/lib/api/types'
import { Skeleton } from '@/components/ui/Skeleton'
import { Section } from '../Section'
import { SectionState } from './SectionState'

function TopColumn({ title, items }: { title: string; items: NamedValueDto[] }) {
  const { t } = useTranslation('stats')
  const max = Math.max(...items.map((i) => i.value))
  return (
    <div>
      <h3 className="mb-2 text-[11px] font-medium tracking-[0.08em] text-ink-3 uppercase">{title}</h3>
      <div className="flex flex-col gap-0.5">
        {items.map((item) => (
          <div key={item.name} className="relative flex items-center justify-between gap-3 rounded-md px-2 py-1.5">
            <div
              className="absolute inset-y-0 left-0 rounded-md bg-accent-soft"
              style={{ width: `${(item.value / max) * 100}%` }}
            />
            <span className="relative min-w-0 truncate text-[13px] text-ink-2">{item.name}</span>
            <span className="relative shrink-0 text-xs tabular-nums text-ink-3">
              {t('tops.series', { count: item.value })}
            </span>
          </div>
        ))}
      </div>
    </div>
  )
}

export function TopsSection({ libraryId }: { libraryId?: string }) {
  const { t } = useTranslation('stats')
  const query = useQuery({
    queryKey: ['stats', 'tops', libraryId ?? 'all'],
    queryFn: () => statsApi.tops(libraryId),
  })

  return (
    <Section title={t('tops.title')}>
      <SectionState query={query} skeleton={<Skeleton className="h-40 w-full" />}>
        {(tops) => {
          const columns = [
            { title: t('tops.authors'), items: tops.topAuthors },
            { title: t('tops.genres'), items: tops.topGenres },
            { title: t('tops.tags'), items: tops.topTags },
          ].filter((c) => c.items.length > 0)
          if (columns.length === 0) return <p className="text-sm text-ink-3">{t('tops.empty')}</p>
          return (
            <div className="grid gap-6 sm:grid-cols-3">
              {columns.map((c) => (
                <TopColumn key={c.title} title={c.title} items={c.items} />
              ))}
            </div>
          )
        }}
      </SectionState>
    </Section>
  )
}
