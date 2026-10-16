import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { statsApi } from '@/lib/api/stats'
import { formatNumber } from '@/lib/utils/format'
import { cn } from '@/lib/utils/cn'
import { Skeleton } from '@/components/ui/Skeleton'
import { Section } from '../Section'
import { SectionState } from './SectionState'

// one hue in three weights: the accent marks progress made, unread stays neutral
const SEGMENT_COLORS: Record<string, string> = {
  read: 'bg-accent',
  inProgress: 'bg-[color-mix(in_srgb,var(--accent)_45%,transparent)]',
  unread: 'bg-[color-mix(in_srgb,var(--ink)_14%,transparent)]',
}

export function StatusSection({ libraryId }: { libraryId?: string }) {
  const { t } = useTranslation('stats')
  const query = useQuery({
    queryKey: ['stats', 'summary', libraryId ?? 'all'],
    queryFn: () => statsApi.summary(libraryId),
  })

  return (
    <Section title={t('status.title')}>
      <SectionState query={query} skeleton={<Skeleton className="h-16 w-full" />}>
        {(s) => {
          const total = s.statusDistribution.reduce((n, d) => n + d.value, 0)
          if (total === 0) return <p className="text-sm text-ink-3">{t('status.empty')}</p>
          return (
            <div>
              <div className="flex h-3 overflow-hidden rounded-full">
                {s.statusDistribution.map((d) =>
                  d.value > 0 ? (
                    <div
                      key={d.name}
                      className={SEGMENT_COLORS[d.name] ?? SEGMENT_COLORS.unread}
                      style={{ width: `${(d.value / total) * 100}%` }}
                    />
                  ) : null,
                )}
              </div>
              <div className="mt-3 flex flex-wrap gap-x-6 gap-y-2">
                {s.statusDistribution.map((d) => (
                  <div key={d.name} className="flex items-center gap-2 text-[13px]">
                    <span className={cn('size-2.5 rounded-full', SEGMENT_COLORS[d.name] ?? SEGMENT_COLORS.unread)} />
                    <span className="text-ink-2">{t(`status.${d.name}`)}</span>
                    <span className="tabular-nums text-ink-3">
                      {formatNumber(d.value)} · {Math.round((d.value / total) * 100)}%
                    </span>
                  </div>
                ))}
              </div>
            </div>
          )
        }}
      </SectionState>
    </Section>
  )
}
