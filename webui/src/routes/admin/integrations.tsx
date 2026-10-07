import { useState, type ReactNode } from 'react'
import { useNavigate } from 'react-router-dom'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { ApiError } from '@/lib/api/client'
import { komfApi } from '@/lib/api/komf'
import type { KomfIntegrationDto, KomfIntegrationState } from '@/lib/api/types'
import { cn } from '@/lib/utils/cn'
import { useDocumentTitle } from '@/lib/hooks/useDocumentTitle'
import { useChanged } from '@/lib/hooks/useChanged'
import { Section } from '@/components/account/Section'
import { Button } from '@/components/ui/Button'
import { Dialog } from '@/components/ui/Dialog'
import { PageHeader } from '@/components/ui/PageHeader'
import { Skeleton } from '@/components/ui/Skeleton'
import { TextField } from '@/components/ui/TextField'

const STATE_PILL: Record<KomfIntegrationState, { labelKey: string; className: string }> = {
  connected: { labelKey: 'integrations.state.connected', className: 'border-accent/40 bg-accent-soft text-accent-strong' },
  pending: { labelKey: 'integrations.state.pending', className: 'border-line bg-raised text-ink-2' },
  error: { labelKey: 'integrations.state.error', className: 'border-danger/40 bg-danger/10 text-danger' },
}

function Pill({ className, children }: { className: string; children: ReactNode }) {
  return (
    <span className={cn('inline-flex items-center rounded-full border px-2.5 py-0.5 text-xs font-medium', className)}>
      {children}
    </span>
  )
}

interface ViolationMessage {
  field: string
  message: string
}

function violationMessages(err: unknown): ViolationMessage[] {
  if (!(err instanceof ApiError)) return []
  const body = err.body as { violations?: { fieldName?: unknown; message?: unknown }[] } | undefined
  if (!body || !Array.isArray(body.violations)) return []
  return body.violations
    .filter((v) => typeof v?.message === 'string')
    .map((v) => ({ field: typeof v.fieldName === 'string' ? v.fieldName : '', message: v.message as string }))
}

function IntegrationForm({ integration }: { integration: KomfIntegrationDto }) {
  const { t } = useTranslation('admin-komf')
  const queryClient = useQueryClient()
  const navigate = useNavigate()
  const [url, setUrl] = useState(integration.url ?? '')
  const [baseUrl, setBaseUrl] = useState(integration.baseUrl ?? window.location.origin)
  // write-only: blank means "keep the stored key" (or "no override" when none is set)
  const [authKey, setAuthKey] = useState('')
  const [confirmOpen, setConfirmOpen] = useState(false)
  const [editing, setEditing] = useState(false)

  // structural sharing keeps the reference stable across refetches, so in-progress edits survive
  if (useChanged([integration])) {
    setUrl(integration.url ?? '')
    setBaseUrl(integration.baseUrl ?? window.location.origin)
    setAuthKey('')
  }

  const invalidate = () => void queryClient.invalidateQueries({ queryKey: ['admin', 'komf-integration'] })

  const connect = useMutation({
    mutationFn: () =>
      komfApi.updateIntegration({
        url: url.trim(),
        baseUrl: baseUrl.trim(),
        ...(authKey.trim() ? { authKey: authKey.trim() } : {}),
      }),
    onSuccess: () => {
      setEditing(false)
      invalidate()
    },
  })
  const clearAuthKey = useMutation({
    // blank clears the override so the kmrs config preset applies again
    mutationFn: () =>
      komfApi.updateIntegration({
        url: (integration.url ?? url).trim(),
        baseUrl: (integration.baseUrl ?? baseUrl).trim(),
        authKey: '',
      }),
    onSuccess: invalidate,
  })
  const disconnect = useMutation({
    mutationFn: komfApi.disconnect,
    onSuccess: () => {
      setConfirmOpen(false)
      setEditing(false)
      invalidate()
    },
  })

  const cancelEdit = () => {
    setUrl(integration.url ?? '')
    setBaseUrl(integration.baseUrl ?? window.location.origin)
    setAuthKey('')
    setEditing(false)
    connect.reset()
    clearAuthKey.reset()
  }

  // a configured integration shows its saved values read-only; Edit unlocks them so a
  // changed URL goes through Reconnect (re-provision) instead of a destructive disconnect
  const locked = integration.configured && !editing

  const violations = connect.error ? violationMessages(connect.error) : []
  const violationFor = (field: string) => violations.find((v) => v.field === field)?.message
  const otherViolations = violations.filter((v) => v.field !== 'url' && v.field !== 'baseUrl' && v.field !== 'authKey')
  const connectError =
    connect.error && violations.length === 0
      ? connect.error instanceof Error
        ? connect.error.message
        : t('integrations.connectFailed')
      : null

  return (
    <div className="space-y-6">
      <Section title="komf">
        <div className="flex flex-col gap-4">
          <div className="flex flex-wrap items-center gap-2">
            {integration.configured && integration.state ? (
              <Pill className={STATE_PILL[integration.state].className}>
                {t(STATE_PILL[integration.state].labelKey)}
              </Pill>
            ) : (
              <Pill className="border-line bg-raised text-ink-2">{t('integrations.notConfigured')}</Pill>
            )}
            {integration.configured && (
              <Pill
                className={
                  integration.komfReachable
                    ? 'border-accent/40 bg-accent-soft text-accent-strong'
                    : 'border-danger/40 bg-danger/10 text-danger'
                }
              >
                {integration.komfReachable ? t('integrations.reachable') : t('integrations.unreachable')}
              </Pill>
            )}
          </div>
          {integration.lastError && <p className="text-sm text-danger">{integration.lastError}</p>}
          <TextField
            label={t('integrations.komfUrl')}
            helper={t('integrations.komfUrlHelper')}
            placeholder="http://komf:8085"
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            error={violationFor('url')}
            disabled={locked}
          />
          <TextField
            label={t('integrations.baseUrl')}
            helper={t('integrations.baseUrlHelper')}
            value={baseUrl}
            onChange={(e) => setBaseUrl(e.target.value)}
            error={violationFor('baseUrl')}
            disabled={locked}
          />
          <TextField
            type="password"
            autoComplete="off"
            label={t('integrations.authKey')}
            helper={t(integration.authKeySet ? 'integrations.authKeyHelperSet' : 'integrations.authKeyHelper')}
            placeholder={integration.authKeySet ? t('integrations.authKeyPlaceholderSet') : undefined}
            value={authKey}
            onChange={(e) => setAuthKey(e.target.value)}
            error={violationFor('authKey')}
            disabled={locked}
            trailing={
              integration.authKeySet && !locked ? (
                <button
                  type="button"
                  className="cursor-pointer text-xs text-ink-3 transition-colors hover:text-danger disabled:opacity-50"
                  disabled={clearAuthKey.isPending}
                  onClick={() => clearAuthKey.mutate()}
                >
                  {t('integrations.clearAuthKey')}
                </button>
              ) : undefined
            }
          />
          {clearAuthKey.isError && (
            <p className="text-sm text-danger">
              {clearAuthKey.error instanceof Error ? clearAuthKey.error.message : t('integrations.connectFailed')}
            </p>
          )}
          <div className="flex items-center gap-2">
            {locked ? (
              <Button variant="primary" onClick={() => setEditing(true)}>
                {t('common:action.edit')}
              </Button>
            ) : (
              <Button variant="primary" loading={connect.isPending} onClick={() => connect.mutate()}>
                {integration.configured ? t('integrations.reconnect') : t('integrations.connect')}
              </Button>
            )}
            {integration.state === 'connected' && (
              <Button variant="secondary" onClick={() => navigate('/admin/integrations/komf')}>
                {t('integrations.configure')}
              </Button>
            )}
            {editing && integration.configured && (
              <Button variant="ghost" onClick={cancelEdit}>
                {t('common:action.cancel')}
              </Button>
            )}
            {integration.configured && (
              <Button variant="danger" onClick={() => setConfirmOpen(true)}>
                {t('integrations.disconnect')}
              </Button>
            )}
          </div>
          {otherViolations.map((v, i) => (
            <p key={i} className="text-sm text-danger">
              {v.field ? `${v.field}: ${v.message}` : v.message}
            </p>
          ))}
          {connectError && <p className="text-sm text-danger">{connectError}</p>}
          {disconnect.isError && (
            <p className="text-sm text-danger">
              {disconnect.error instanceof Error ? disconnect.error.message : t('integrations.disconnectFailed')}
            </p>
          )}
        </div>
      </Section>

      <Dialog open={confirmOpen} onOpenChange={setConfirmOpen} title={t('integrations.disconnectTitle')} size="sm">
        <div className="px-5 py-4">
          <p className="text-sm text-ink-2">{t('integrations.disconnectBody')}</p>
        </div>
        <div className="flex justify-end gap-2 border-t border-line px-5 py-3.5">
          <Button variant="ghost" onClick={() => setConfirmOpen(false)}>
            {t('common:action.cancel')}
          </Button>
          <Button variant="danger" loading={disconnect.isPending} onClick={() => disconnect.mutate()}>
            {t('integrations.disconnect')}
          </Button>
        </div>
      </Dialog>
    </div>
  )
}

export function AdminIntegrationsPage() {
  const { t } = useTranslation('admin-komf')
  const query = useQuery({ queryKey: ['admin', 'komf-integration'], queryFn: komfApi.getIntegration })

  useDocumentTitle(t('layout:nav.integrations'))

  return (
    <div className="max-w-3xl">
      <PageHeader title={t('layout:nav.integrations')} subtitle={t('integrations.subtitle')} />
      {query.isLoading && (
        <div className="space-y-6">
          <Skeleton className="h-72 w-full rounded-xl" />
        </div>
      )}
      {query.isLoadingError && (
        <div className="flex items-center gap-3 rounded-xl border border-line bg-surface p-5">
          <p className="text-sm text-danger">
            {query.error instanceof Error ? query.error.message : t('integrations.loadFailed')}
          </p>
          <Button size="sm" onClick={() => void query.refetch()}>
            {t('common:action.retry')}
          </Button>
        </div>
      )}
      {query.data && <IntegrationForm integration={query.data} />}
    </div>
  )
}
