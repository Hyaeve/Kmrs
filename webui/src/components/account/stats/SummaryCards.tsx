import type { ReactNode } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { BookOpen, Books, CalendarCheck, ChartLine, Clock, FileText, Flame, Trophy } from '@phosphor-icons/react'
import { statsApi } from '@/lib/api/stats'
import { formatNumber, relativeTime } from '@/lib/utils/format'
import { Skeleton } from '@/components/ui/Skeleton'
import { SectionState } from './SectionState'

function StatCard({ icon, label, value, sub }: { icon: ReactNode; label: string; value: ReactNode; sub?: ReactNode }) {
  return (
    <div className="rounded-xl border border-line bg-surface p-4">
      <div className="flex items-center gap-1.5 text-xs text-ink-3">
        {icon}
        {label}
      </div>
      <p className="mt-2 truncate text-xl font-semibold text-ink">{value}</p>
      {sub && <p className="mt-0.5 truncate text-xs text-ink-3">{sub}</p>}
    </div>
  )
}

export function SummaryCards({ libraryId }: { libraryId?: string }) {
  const { t } = useTranslation('stats')
  const query = useQuery({
    queryKey: ['stats', 'summary', libraryId ?? 'all'],
    queryFn: () => statsApi.summary(libraryId),
  })

  return (
    <SectionState
      query={query}
      skeleton={
        <div className="grid grid-cols-2 gap-4 sm:grid-cols-4">
          {Array.from({ length: 8 }, (_, i) => (
            <Skeleton key={i} className="h-[78px]" />
          ))}
        </div>
      }
    >
      {(s) => (
        <div className="grid grid-cols-2 gap-4 sm:grid-cols-4">
          <StatCard
            icon={<Books className="size-3.5" />}
            label={t('summary.booksRead')}
            value={formatNumber(s.booksCompleted)}
            sub={t('summary.ofTotal', { count: s.totalBooks, total: formatNumber(s.totalBooks) })}
          />
          <StatCard
            icon={<BookOpen className="size-3.5" />}
            label={t('summary.inProgress')}
            value={formatNumber(s.booksStarted - s.booksCompleted)}
          />
          <StatCard icon={<FileText className="size-3.5" />} label={t('summary.pagesRead')} value={formatNumber(s.pagesRead)} />
          <StatCard
            icon={<CalendarCheck className="size-3.5" />}
            label={t('summary.readingDays')}
            value={formatNumber(s.readingDays)}
          />
          <StatCard
            icon={<Flame className="size-3.5" />}
            label={t('summary.currentStreak')}
            value={t('summary.days', { count: s.currentStreakDays })}
          />
          <StatCard
            icon={<Trophy className="size-3.5" />}
            label={t('summary.longestStreak')}
            value={t('summary.days', { count: s.longestStreakDays })}
          />
          <StatCard
            icon={<ChartLine className="size-3.5" />}
            label={t('summary.avgPages')}
            value={formatNumber(s.averagePagesPerBook)}
          />
          <StatCard
            icon={<Clock className="size-3.5" />}
            label={t('summary.lastRead')}
            value={s.lastReadAt ? relativeTime(s.lastReadAt) : t('summary.never')}
          />
        </div>
      )}
    </SectionState>
  )
}
