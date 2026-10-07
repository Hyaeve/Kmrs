import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { WarningCircle } from '@phosphor-icons/react'
import { clientSettingsApi } from '@/lib/api/clientSettings'
import { usersApi } from '@/lib/api/users'
import { useDocumentTitle } from '@/lib/hooks/useDocumentTitle'
import { Button } from '@/components/ui/Button'
import { EmptyState } from '@/components/ui/EmptyState'
import { PageHeader } from '@/components/ui/PageHeader'
import { Skeleton } from '@/components/ui/Skeleton'
import { Switch } from '@/components/ui/Switch'
import { Section } from '@/components/account/Section'

// server-side keys must be all-lowercase dotted segments
export const HIDE_PASSWORD_KEY = 'webui.login.hide-password'

export function AdminUiPage() {
  const { t } = useTranslation('admin-settings')
  const queryClient = useQueryClient()

  useDocumentTitle(t('ui.title'))

  const settingsQuery = useQuery({ queryKey: ['admin', 'client-settings', 'global'], queryFn: clientSettingsApi.listGlobal })
  // shown only as context: the toggle is a no-op without providers
  const providersQuery = useQuery({ queryKey: ['oauth2-providers'], queryFn: usersApi.oauth2Providers })

  const hidePassword = settingsQuery.data?.[HIDE_PASSWORD_KEY]?.value === 'true'
  const providerCount = providersQuery.data?.length ?? 0

  const save = useMutation({
    mutationFn: (hide: boolean) =>
      clientSettingsApi.saveGlobal({
        // allowUnauthorized: the login page is anonymous and must be able to read this
        [HIDE_PASSWORD_KEY]: { value: String(hide), allowUnauthorized: true },
      }),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: ['admin', 'client-settings', 'global'] }),
  })

  return (
    <div className="max-w-3xl">
      <PageHeader title={t('ui.title')} subtitle={t('ui.subtitle')} />

      {settingsQuery.isPending ? (
        <Skeleton className="h-28 rounded-xl" />
      ) : settingsQuery.isLoadingError ? (
        <EmptyState
          icon={<WarningCircle />}
          title={t('ui.loadFailed')}
          body={settingsQuery.error instanceof Error ? settingsQuery.error.message : t('ui.errorFallback')}
          action={<Button onClick={() => settingsQuery.refetch()}>{t('common:action.retry')}</Button>}
        />
      ) : (
        <Section title={t('ui.login')}>
          <div className="flex flex-wrap items-center justify-between gap-x-6 gap-y-2.5">
            <div className="min-w-0 max-w-md">
              <p className="text-sm text-ink-2">{t('ui.hidePassword')}</p>
              <p className="mt-0.5 text-xs text-ink-3">
                {providersQuery.data
                  ? t('ui.hidePasswordHelperWithCount', { count: providerCount })
                  : t('ui.hidePasswordHelper')}
              </p>
            </div>
            <Switch
              checked={hidePassword}
              onCheckedChange={(v) => save.mutate(v)}
              disabled={save.isPending}
              label={t('ui.hidePassword')}
            />
          </div>
          {hidePassword && providersQuery.data && providerCount === 0 && (
            <p className="mt-3 text-xs text-accent-strong">{t('ui.noProviderWarning')}</p>
          )}
          {save.isError && (
            <p className="mt-3 text-sm text-danger">
              {save.error instanceof Error ? save.error.message : t('ui.saveFailed')}
            </p>
          )}
        </Section>
      )}
    </div>
  )
}
