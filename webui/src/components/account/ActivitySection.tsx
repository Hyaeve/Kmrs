import { useState } from 'react'
import { keepPreviousData, useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { CaretLeft, CaretRight } from '@phosphor-icons/react'
import { usersApi } from '@/lib/api/users'
import { relativeTime } from '@/lib/utils/format'
import { cn } from '@/lib/utils/cn'
import { Button } from '@/components/ui/Button'
import { Skeleton } from '@/components/ui/Skeleton'
import { Section } from './Section'

const PAGE_SIZE = 20

export function ActivitySection() {
  const { t } = useTranslation('account')
  const [page, setPage] = useState(0)
  const query = useQuery({
    queryKey: ['account', 'activity', page],
    queryFn: () => usersApi.authenticationActivity({ page, size: PAGE_SIZE }),
    placeholderData: keepPreviousData,
  })

  return (
    <Section title={t('security.activity.title')}>
      {query.isLoading && (
        <div className="flex flex-col gap-2.5">
          {Array.from({ length: 5 }, (_, i) => (
            <Skeleton key={i} className="h-7 w-full" />
          ))}
        </div>
      )}
      {query.isLoadingError && (
        <div className="flex items-center gap-3">
          <p className="text-sm text-danger">
            {query.error instanceof Error ? query.error.message : t('security.activity.failed')}
          </p>
          <Button size="sm" onClick={() => void query.refetch()}>
            {t('common:action.retry')}
          </Button>
        </div>
      )}
      {query.data && (
        <>
          {query.data.content.length === 0 ? (
            <p className="text-sm text-ink-3">{t('security.activity.empty')}</p>
          ) : (
            <div className="overflow-x-auto">
              <table className="w-full min-w-[560px] text-sm">
                <thead>
                  <tr className="text-left text-xs text-ink-3">
                    <th className="pb-2 pr-4 font-medium">{t('security.activity.when')}</th>
                    <th className="pb-2 pr-4 font-medium">{t('security.activity.ip')}</th>
                    <th className="pb-2 pr-4 font-medium">{t('security.activity.source')}</th>
                    <th className="pb-2 pr-4 font-medium">{t('security.activity.status')}</th>
                    <th className="pb-2 font-medium">{t('security.activity.agent')}</th>
                  </tr>
                </thead>
                <tbody>
                  {query.data.content.map((a, i) => (
                    <tr key={i} className="border-t border-line">
                      <td className="py-2.5 pr-4 whitespace-nowrap text-ink-2">{relativeTime(a.dateTime)}</td>
                      <td className="py-2.5 pr-4 font-mono text-xs whitespace-nowrap text-ink-2">{a.ip || '-'}</td>
                      <td className="py-2.5 pr-4 whitespace-nowrap text-ink-2">{a.source || '-'}</td>
                      <td className={cn('py-2.5 pr-4 whitespace-nowrap', a.success ? 'text-ink-2' : 'text-danger')}>
                        {a.success ? t('security.activity.success') : t('security.activity.statusFailed')}
                      </td>
                      <td className="max-w-48 truncate py-2.5 text-ink-3" title={a.userAgent}>
                        {a.userAgent || '-'}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
          <div className="mt-4 flex items-center justify-between">
            <span className="font-mono text-xs text-ink-3">
              {t('security.activity.page', { current: query.data.number + 1, total: Math.max(query.data.totalPages, 1) })}
            </span>
            <div className="flex gap-2">
              <Button size="sm" onClick={() => setPage((p) => p - 1)} disabled={page === 0 || query.isFetching}>
                <CaretLeft className="size-4" />
                {t('security.activity.previous')}
              </Button>
              <Button
                size="sm"
                onClick={() => setPage((p) => p + 1)}
                disabled={query.data.last || query.isFetching}
              >
                {t('security.activity.next')}
                <CaretRight className="size-4" />
              </Button>
            </div>
          </div>
        </>
      )}
    </Section>
  )
}
