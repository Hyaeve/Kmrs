import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { statsApi } from '@/lib/api/stats'
import { formatNumber, hourLabel, weekdayShort } from '@/lib/utils/format'
import { Skeleton } from '@/components/ui/Skeleton'
import { Tooltip } from '@/components/ui/Tooltip'
import { Section } from '../Section'
import { SectionState } from './SectionState'

function Bars({ values, labels, tooltips }: { values: number[]; labels: (string | null)[]; tooltips: string[] }) {
  const { t } = useTranslation('stats')
  const max = Math.max(...values)
  return (
    <div className="flex gap-1">
      {values.map((v, i) => (
        <Tooltip key={i} content={t('chart.tooltip', { label: tooltips[i], value: formatNumber(v) })}>
          <div className="flex h-28 min-w-0 flex-1 flex-col">
            <div className="flex flex-1 items-end border-b border-line">
              {v > 0 && (
                <div
                  className="w-full rounded-t-[3px] bg-[color-mix(in_srgb,var(--accent)_75%,transparent)] transition-colors hover:bg-accent"
                  style={{ height: `${Math.max((v / max) * 100, 2)}%` }}
                />
              )}
            </div>
            <div className="mt-1 h-3.5 text-center text-[10px] whitespace-nowrap text-ink-3">{labels[i]}</div>
          </div>
        </Tooltip>
      ))}
    </div>
  )
}

export function ActivityCharts({ libraryId }: { libraryId?: string }) {
  const { t } = useTranslation('stats')
  const tzOffsetMinutes = -new Date().getTimezoneOffset()
  const query = useQuery({
    queryKey: ['stats', 'activity', libraryId ?? 'all', tzOffsetMinutes],
    queryFn: () => statsApi.activity(libraryId, tzOffsetMinutes),
  })

  return (
    <div className="grid gap-4 sm:grid-cols-2">
      <Section title={t('weekday.title')}>
        <SectionState query={query} skeleton={<Skeleton className="h-28 w-full" />}>
          {(a) => (
            <Bars
              values={a.weekdayDistribution}
              labels={a.weekdayDistribution.map((_, i) => weekdayShort(i))}
              tooltips={a.weekdayDistribution.map((_, i) => weekdayShort(i))}
            />
          )}
        </SectionState>
      </Section>
      <Section title={t('hourly.title')}>
        <SectionState query={query} skeleton={<Skeleton className="h-28 w-full" />}>
          {(a) => (
            <Bars
              values={a.hourlyDistribution}
              labels={a.hourlyDistribution.map((_, i) => (i % 6 === 0 ? hourLabel(i) : null))}
              tooltips={a.hourlyDistribution.map((_, i) => hourLabel(i))}
            />
          )}
        </SectionState>
      </Section>
    </div>
  )
}
