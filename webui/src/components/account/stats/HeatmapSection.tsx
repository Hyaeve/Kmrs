import { useMemo, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { statsApi } from '@/lib/api/stats'
import { formatDay, monthShort, weekdayShort } from '@/lib/utils/format'
import { cn } from '@/lib/utils/cn'
import { SegmentedControl } from '@/components/ui/SegmentedControl'
import { Skeleton } from '@/components/ui/Skeleton'
import { Tooltip } from '@/components/ui/Tooltip'
import { SectionState } from './SectionState'

type RangeKey = '3m' | '6m' | '1y'
const RANGE_MONTHS: Record<RangeKey, number> = { '3m': 3, '6m': 6, '1y': 12 }

const CELL = 12
const GAP = 3
const PITCH = CELL + GAP

// the empty level must stay visible on both themes, so it derives from ink, not raised
const LEVELS = [
  'bg-[color-mix(in_srgb,var(--ink)_7%,transparent)]',
  'bg-[color-mix(in_srgb,var(--accent)_30%,transparent)]',
  'bg-[color-mix(in_srgb,var(--accent)_55%,transparent)]',
  'bg-[color-mix(in_srgb,var(--accent)_80%,transparent)]',
  'bg-accent',
]

interface DayCell {
  day: string
  pages: number
  books: number
}

function dayOf(date: Date): string {
  return date.toISOString().slice(0, 10)
}

export function HeatmapSection({ libraryId }: { libraryId?: string }) {
  const { t } = useTranslation('stats')
  const tzOffsetMinutes = -new Date().getTimezoneOffset()
  const query = useQuery({
    queryKey: ['stats', 'activity', libraryId ?? 'all', tzOffsetMinutes],
    queryFn: () => statsApi.activity(libraryId, tzOffsetMinutes),
  })
  const [range, setRange] = useState<RangeKey>('3m')

  const { weeks, quartile, hasData } = useMemo(() => {
    const byDay = new Map((query.data?.readingTimeSeries ?? []).map((p) => [p.date, p]))
    // series days are UTC, so the grid runs on UTC days to keep cells aligned with them
    const now = new Date()
    const end = new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate()))
    const cursor = new Date(end)
    cursor.setUTCMonth(cursor.getUTCMonth() - RANGE_MONTHS[range])
    // columns start on Sunday, matching the weekday buckets' indexing
    cursor.setUTCDate(cursor.getUTCDate() - cursor.getUTCDay())

    const weeks: DayCell[][] = []
    let maxPages = 0
    let hasData = false
    while (cursor <= end) {
      const week: DayCell[] = []
      for (let dow = 0; dow < 7 && cursor <= end; dow++) {
        const day = dayOf(cursor)
        const point = byDay.get(day)
        if (point) hasData = true
        if (point && point.pagesRead > maxPages) maxPages = point.pagesRead
        week.push({ day, pages: point?.pagesRead ?? 0, books: point?.booksCompleted ?? 0 })
        cursor.setUTCDate(cursor.getUTCDate() + 1)
      }
      weeks.push(week)
    }
    return { weeks, quartile: maxPages / 4, hasData }
  }, [query.data, range])

  // a label loses to the next one when they would land closer than 3 columns:
  // the partial month at the range edge yields to the month that fills the grid
  const monthLabels = weeks
    .map((week, wi) => ({ wi, day: week[0].day, month: week[0].day.slice(0, 7) }))
    .filter((c, i, arr) => i === 0 || arr[i - 1].month !== c.month)
    .filter((c, i, arr) => i === arr.length - 1 || arr[i + 1].wi - c.wi >= 3)

  const level = (pages: number) => (pages === 0 || quartile === 0 ? 0 : Math.min(4, Math.ceil(pages / quartile)))

  return (
    <section className="min-w-0 rounded-xl border border-line bg-surface p-5">
      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <h2 className="text-[15px] font-semibold text-ink">{t('heatmap.title')}</h2>
        <SegmentedControl
          size="sm"
          options={[
            { value: '3m', label: t('heatmap.range3m') },
            { value: '6m', label: t('heatmap.range6m') },
            { value: '1y', label: t('heatmap.range1y') },
          ]}
          value={range}
          onChange={setRange}
        />
      </div>
      <SectionState query={query} skeleton={<Skeleton className="h-32 w-full" />}>
        {() =>
          !hasData ? (
            <p className="py-8 text-center text-sm text-ink-3">{t('heatmap.empty')}</p>
          ) : (
            <div className="overflow-x-auto">
              <div className="inline-block">
                <div className="relative mb-1 h-4" style={{ marginLeft: 32 }}>
                  {monthLabels.map(({ wi, day }) => (
                    <span key={wi} className="absolute text-[10px] text-ink-3" style={{ left: wi * PITCH }}>
                      {monthShort(day)}
                    </span>
                  ))}
                </div>
                <div className="flex gap-2">
                  <div className="flex w-6 flex-col" style={{ gap: GAP }}>
                    {Array.from({ length: 7 }, (_, dow) => (
                      <div key={dow} className="flex items-center text-[10px] text-ink-3" style={{ height: CELL }}>
                        {dow % 2 === 1 ? weekdayShort(dow) : null}
                      </div>
                    ))}
                  </div>
                  {weeks.map((week, wi) => (
                    <div key={wi} className="flex flex-col" style={{ gap: GAP }}>
                      {week.map((cell) => (
                        <Tooltip
                          key={cell.day}
                          content={
                            <div className="flex flex-col gap-0.5">
                              <div className="font-medium">{formatDay(cell.day)}</div>
                              <div>
                                {cell.pages > 0 ? t('heatmap.pages', { count: cell.pages }) : t('heatmap.noActivity')}
                              </div>
                              {cell.books > 0 && <div>{t('heatmap.completed', { count: cell.books })}</div>}
                            </div>
                          }
                        >
                          <div
                            tabIndex={0}
                            aria-label={t('heatmap.dayLabel', {
                              date: formatDay(cell.day),
                              activity:
                                cell.pages > 0 ? t('heatmap.pages', { count: cell.pages }) : t('heatmap.noActivity'),
                            })}
                            className={cn('rounded-[3px]', LEVELS[level(cell.pages)])}
                            style={{ width: CELL, height: CELL }}
                          />
                        </Tooltip>
                      ))}
                    </div>
                  ))}
                </div>
                <div className="mt-2 flex items-center justify-end gap-1 text-[10px] text-ink-3">
                  {t('heatmap.less')}
                  {LEVELS.map((c) => (
                    <span key={c} className={cn('inline-block rounded-[3px]', c)} style={{ width: CELL, height: CELL }} />
                  ))}
                  {t('heatmap.more')}
                </div>
              </div>
            </div>
          )
        }
      </SectionState>
    </section>
  )
}
