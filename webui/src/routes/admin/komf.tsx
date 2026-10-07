import { useEffect, useRef } from 'react'
import { Link, useSearchParams } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { ApiError } from '@/lib/api/client'
import { komfApi } from '@/lib/api/komf'
import { librariesApi } from '@/lib/api/libraries'
import { useDocumentTitle } from '@/lib/hooks/useDocumentTitle'
import { showToast } from '@/lib/store/toast'
import { Button } from '@/components/ui/Button'
import { EmptyState } from '@/components/ui/EmptyState'
import { PageHeader } from '@/components/ui/PageHeader'
import { Skeleton } from '@/components/ui/Skeleton'
import { KomfConfigForm } from '@/components/admin/komf/KomfConfigForm'

export function AdminKomfPage() {
  const { t } = useTranslation('admin-komf')
  const queryClient = useQueryClient()
  const query = useQuery({ queryKey: ['admin', 'komf-config'], queryFn: komfApi.getConfig })
  const librariesQuery = useQuery({ queryKey: ['libraries'], queryFn: librariesApi.list })
  const [searchParams, setSearchParams] = useSearchParams()

  useDocumentTitle('komf')

  // the OAuth callback lands on /?oauth=... and RootRedirect forwards it here
  // StrictMode double-invokes the landing effect in dev; the ref keeps it to one toast per landing
  const oauthHandled = useRef(false)
  useEffect(() => {
    const oauth = searchParams.get('oauth')
    if (oauth === null || oauthHandled.current) return
    oauthHandled.current = true
    if (oauth === 'success') showToast(t('oauth.success'))
    else if (oauth === 'error') showToast(searchParams.get('message') || t('oauth.error'))
    const next = new URLSearchParams(searchParams)
    next.delete('oauth')
    next.delete('message')
    setSearchParams(next, { replace: true })
    void queryClient.invalidateQueries({ queryKey: ['admin', 'komf-oauth'] })
  }, [searchParams, setSearchParams, queryClient, t])

  const notConnected = query.error instanceof ApiError && query.error.status === 409
  const loadError = (query.isLoadingError ? query.error : null) ?? (librariesQuery.isLoadingError ? librariesQuery.error : null)

  return (
    <div className="max-w-3xl">
      <PageHeader title="komf" subtitle={t('page.subtitle')} />
      {(query.isLoading || librariesQuery.isLoading) && (
        <div className="space-y-6">
          <Skeleton className="h-40 w-full rounded-xl" />
          <Skeleton className="h-72 w-full rounded-xl" />
          <Skeleton className="h-72 w-full rounded-xl" />
        </div>
      )}
      {notConnected ? (
        <EmptyState
          title={t('page.notConnectedTitle')}
          body={t('page.notConnectedBody')}
          action={
            <Link to="/admin/integrations">
              <Button variant="primary">{t('page.goToIntegrations')}</Button>
            </Link>
          }
        />
      ) : (
        loadError && (
          <div className="flex items-center gap-3 rounded-xl border border-line bg-surface p-5">
            <p className="text-sm text-danger">
              {loadError instanceof Error ? loadError.message : t('page.loadFailed')}
            </p>
            <Button
              size="sm"
              onClick={() => {
                void query.refetch()
                void librariesQuery.refetch()
              }}
            >
              {t('common:action.retry')}
            </Button>
          </div>
        )
      )}
      {query.data && librariesQuery.data && <KomfConfigForm config={query.data} libraries={librariesQuery.data} />}
    </div>
  )
}
