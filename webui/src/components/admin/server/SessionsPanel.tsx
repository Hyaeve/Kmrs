import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { actuatorApi } from '@/lib/api/settings'
import { relativeTime } from '@/lib/utils/format'
import { Button } from '@/components/ui/Button'
import { Chip } from '@/components/ui/Chip'
import { Skeleton } from '@/components/ui/Skeleton'
import { Section } from '@/components/account/Section'

const SESSION_LIMIT = 10

export function SessionsPanel() {
  const { t } = useTranslation('admin-settings')
  const queryClient = useQueryClient()
  const query = useQuery({ queryKey: ['admin', 'sessions'], queryFn: () => actuatorApi.sessions() })

  const kick = useMutation({
    mutationFn: (id: string) => actuatorApi.deleteSession(id),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['admin', 'sessions'] }),
  })

  const sessions = [...(query.data?.sessions ?? [])].sort((a, b) => b.lastAccessedTime.localeCompare(a.lastAccessedTime))
  const [showAll, setShowAll] = useState(false)
  const visible = showAll ? sessions : sessions.slice(0, SESSION_LIMIT)

  return (
    <Section title={t('sessions.title')}>
      {query.isLoading && (
        <div className="flex flex-col gap-2.5">
          {Array.from({ length: 3 }, (_, i) => (
            <Skeleton key={i} className="h-7 w-full" />
          ))}
        </div>
      )}
      {query.isLoadingError && (
        <div className="flex items-center gap-3">
          <p className="text-sm text-danger">
            {query.error instanceof Error ? query.error.message : t('sessions.loadFailed')}
          </p>
          <Button size="sm" onClick={() => void query.refetch()}>
            {t('common:action.retry')}
          </Button>
        </div>
      )}
      {query.data && (
        <>
          {sessions.length === 0 ? (
            <p className="text-sm text-ink-3">{t('sessions.empty')}</p>
          ) : (
            <>
              <div className="overflow-x-auto">
                <table className="w-full min-w-[480px] text-sm">
                  <thead>
                    <tr className="text-left text-xs text-ink-3">
                      <th className="pb-2 pr-4 font-medium">{t('sessions.session')}</th>
                      <th className="pb-2 pr-4 font-medium">{t('sessions.created')}</th>
                      <th className="pb-2 pr-4 font-medium">{t('sessions.lastActive')}</th>
                      <th className="pb-2 pr-4 font-medium">{t('sessions.status')}</th>
                      <th className="pb-2 text-right font-medium">
                        <span className="sr-only">{t('sessions.actions')}</span>
                      </th>
                    </tr>
                  </thead>
                  <tbody>
                    {visible.map((s) => (
                      <tr key={s.id} className="border-t border-line">
                        <td className="py-2.5 pr-4 font-mono text-xs whitespace-nowrap text-ink-2" title={s.id}>
                          {s.id.length > 12 ? `${s.id.slice(0, 12)}…` : s.id}
                        </td>
                        <td className="py-2.5 pr-4 whitespace-nowrap text-ink-2">{relativeTime(s.creationTime)}</td>
                        <td className="py-2.5 pr-4 whitespace-nowrap text-ink-2">{relativeTime(s.lastAccessedTime)}</td>
                        <td className="py-2.5 pr-4">
                          {s.expired ? (
                            <Chip className="border-danger/40 text-danger">{t('sessions.expired')}</Chip>
                          ) : (
                            <Chip>{t('sessions.active')}</Chip>
                          )}
                        </td>
                        <td className="py-2.5 text-right">
                          <Button size="sm" variant="danger" onClick={() => kick.mutate(s.id)} disabled={kick.isPending}>
                            {t('sessions.kick')}
                          </Button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              {sessions.length > SESSION_LIMIT && (
                <button
                  type="button"
                  onClick={() => setShowAll((v) => !v)}
                  className="mt-2 cursor-pointer text-sm text-accent-strong hover:underline"
                >
                  {showAll ? t('common:action.showLess') : t('sessions.showAll', { count: sessions.length })}
                </button>
              )}
            </>
          )}
          <p className="mt-3 text-xs text-ink-3">{t('sessions.kickHint')}</p>
          {kick.isError && (
            <p className="mt-2 text-sm text-danger">
              {kick.error instanceof Error ? kick.error.message : t('sessions.kickFailed')}
            </p>
          )}
        </>
      )}
    </Section>
  )
}
