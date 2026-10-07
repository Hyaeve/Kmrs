import { useState } from 'react'
import { keepPreviousData, useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { CaretLeft, CaretRight } from '@phosphor-icons/react'
import { usersApi } from '@/lib/api/users'
import i18n from '@/lib/i18n'
import { cn } from '@/lib/utils/cn'
import { relativeTime } from '@/lib/utils/format'
import { Button } from '@/components/ui/Button'
import { Skeleton } from '@/components/ui/Skeleton'
import { Tooltip } from '@/components/ui/Tooltip'

const PAGE_SIZE = 20

function absoluteTime(iso: string): string {
  return new Date(iso).toLocaleString(i18n.language, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  })
}

export function ActivitySection() {
  const { t } = useTranslation('admin-users')
  const [page, setPage] = useState(0)
  const query = useQuery({
    queryKey: ['admin', 'authentication-activity', page],
    queryFn: () => usersApi.allAuthenticationActivity({ page, size: PAGE_SIZE }),
    placeholderData: keepPreviousData,
  })

  return (
    <section className="mt-10">
      <div className="mb-3 flex items-baseline justify-between gap-4">
        <h2 className="text-[15px] font-semibold text-ink">{t('activity.title')}</h2>
        {query.data && <span className="text-xs text-ink-3">{t('activity.events', { count: query.data.totalElements })}</span>}
      </div>

      {query.isLoading && (
        <div className="flex flex-col gap-2.5 rounded-xl border border-line bg-surface p-4">
          {Array.from({ length: 5 }, (_, i) => (
            <Skeleton key={i} className="h-7 w-full" />
          ))}
        </div>
      )}

      {query.isLoadingError && (
        <div className="flex items-center gap-3 rounded-xl border border-line bg-surface px-4 py-6">
          <p className="text-sm text-danger">
            {query.error instanceof Error ? query.error.message : t('activity.loadFailed')}
          </p>
          <Button size="sm" onClick={() => void query.refetch()}>
            {t('common:action.retry')}
          </Button>
        </div>
      )}

      {query.data && (
        <div className="overflow-hidden rounded-xl border border-line bg-surface">
          {query.data.content.length === 0 ? (
            <p className="px-4 py-10 text-center text-sm text-ink-3">{t('activity.empty')}</p>
          ) : (
            <div className="overflow-x-auto">
              <table className="w-full min-w-[760px] text-sm">
                <thead>
                  <tr className="border-b border-line text-left text-xs text-ink-3">
                    <th className="px-4 py-3 font-medium">{t('account:security.activity.when')}</th>
                    <th className="px-4 py-3 font-medium">{t('user')}</th>
                    <th className="px-4 py-3 font-medium">{t('account:security.activity.ip')}</th>
                    <th className="px-4 py-3 font-medium">{t('account:security.activity.source')}</th>
                    <th className="px-4 py-3 font-medium">{t('account:security.activity.status')}</th>
                    <th className="px-4 py-3 font-medium">{t('account:security.activity.agent')}</th>
                  </tr>
                </thead>
                <tbody>
                  {query.data.content.map((a, i) => (
                    <tr key={`${a.dateTime}-${i}`} className="border-t border-line first:border-t-0">
                      <td className="px-4 py-2.5 whitespace-nowrap text-ink-2">
                        <Tooltip content={absoluteTime(a.dateTime)}>
                          <span>{relativeTime(a.dateTime)}</span>
                        </Tooltip>
                      </td>
                      <td className="max-w-52 truncate px-4 py-2.5 text-ink-2">{a.email ?? '—'}</td>
                      <td className="px-4 py-2.5 font-mono text-xs whitespace-nowrap text-ink-2">{a.ip || '—'}</td>
                      <td className="px-4 py-2.5 whitespace-nowrap text-ink-2">
                        {a.source || '—'}
                        {a.apiKeyComment && <span className="text-ink-3"> · {a.apiKeyComment}</span>}
                      </td>
                      <td className={cn('px-4 py-2.5 whitespace-nowrap', a.success ? 'text-ink-2' : 'text-danger')}>
                        {a.success || !a.error ? (
                          a.success ? (
                            t('account:security.activity.success')
                          ) : (
                            t('account:security.activity.statusFailed')
                          )
                        ) : (
                          <Tooltip content={a.error}>
                            <span className="underline decoration-danger/60 decoration-dotted underline-offset-4">
                              {t('account:security.activity.statusFailed')}
                            </span>
                          </Tooltip>
                        )}
                      </td>
                      <td className="max-w-56 truncate px-4 py-2.5 text-ink-3">
                        {a.userAgent ? (
                          <Tooltip content={a.userAgent}>
                            <span>{a.userAgent}</span>
                          </Tooltip>
                        ) : (
                          '—'
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
          <div className="flex items-center justify-between border-t border-line px-4 py-3">
            <span className="font-mono text-xs text-ink-3">
              {t('account:security.activity.page', {
                current: query.data.number + 1,
                total: Math.max(query.data.totalPages, 1),
              })}
            </span>
            <div className="flex gap-2">
              <Button size="sm" onClick={() => setPage((p) => p - 1)} disabled={page === 0 || query.isFetching}>
                <CaretLeft className="size-4" />
                {t('account:security.activity.previous')}
              </Button>
              <Button
                size="sm"
                onClick={() => setPage((p) => p + 1)}
                disabled={query.data.last || query.isFetching}
              >
                {t('account:security.activity.next')}
                <CaretRight className="size-4" />
              </Button>
            </div>
          </div>
        </div>
      )}
    </section>
  )
}
