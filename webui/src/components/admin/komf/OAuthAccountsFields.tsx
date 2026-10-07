import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { ApiError } from '@/lib/api/client'
import { komfApi } from '@/lib/api/komf'
import type { KomfOAuthProvider } from '@/lib/api/types'
import { showToast } from '@/lib/store/toast'
import { Button } from '@/components/ui/Button'
import { Skeleton } from '@/components/ui/Skeleton'
import { PROVIDER_LABELS } from './draft'
import { FormRow } from './FormRow'

const OAUTH_PROVIDERS: ReadonlyArray<{ id: KomfOAuthProvider; label: string }> = [
  { id: 'anilist', label: PROVIDER_LABELS.aniList },
  { id: 'mal', label: PROVIDER_LABELS.mal },
  { id: 'bangumi', label: PROVIDER_LABELS.bangumi },
  { id: 'mangabaka', label: PROVIDER_LABELS.mangaBaka },
]

export function OAuthAccountsFields() {
  const { t } = useTranslation('admin-komf')
  const queryClient = useQueryClient()
  const query = useQuery({
    queryKey: ['admin', 'komf-oauth'],
    retry: false,
    queryFn: () =>
      Promise.all(OAUTH_PROVIDERS.map(async (p) => ({ provider: p, status: await komfApi.oauthStatus(p.id) }))),
  })
  const logout = useMutation({
    mutationFn: (provider: KomfOAuthProvider) => komfApi.oauthLogout(provider),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['admin', 'komf-oauth'] }),
    onError: (e) => showToast(e instanceof Error ? e.message : t('oauth.logoutFailed')),
  })

  // a komf predating the OAuth routes 404s every status call
  const komfTooOld = query.error instanceof ApiError && query.error.status === 404

  return (
    <div>
      <p className="text-xs text-ink-3">{t('oauth.description')}</p>
      {query.isLoading && <Skeleton className="mt-3 h-8 w-full" />}
      {komfTooOld ? (
        <p className="mt-3 text-sm text-ink-3">{t('oauth.upgradeHint')}</p>
      ) : (
        query.isLoadingError && (
          <div className="mt-3 flex items-center gap-3">
            <p className="text-sm text-danger">
              {query.error instanceof Error ? query.error.message : t('oauth.loadFailed')}
            </p>
            <Button size="sm" onClick={() => void query.refetch()}>
              {t('common:action.retry')}
            </Button>
          </div>
        )
      )}
      {query.data && (
        <div className="mt-1">
          {query.data.map(({ provider: p, status }) => (
            <FormRow
              key={p.id}
              label={p.label}
              helper={
                status.logged_in
                  ? status.username
                    ? t('oauth.loggedInAs', { name: status.username })
                    : t('oauth.loggedIn')
                  : t('oauth.notLoggedIn')
              }
            >
              {status.logged_in ? (
                <Button
                  size="sm"
                  loading={logout.isPending && logout.variables === p.id}
                  onClick={() => logout.mutate(p.id)}
                >
                  {t('oauth.logout')}
                </Button>
              ) : (
                <Button size="sm" onClick={() => window.location.assign(komfApi.oauthStartUrl(p.id))}>
                  {t('oauth.login')}
                </Button>
              )}
            </FormRow>
          ))}
        </div>
      )}
    </div>
  )
}
