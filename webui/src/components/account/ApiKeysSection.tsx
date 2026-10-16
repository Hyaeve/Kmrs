import { useMemo, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Trans, useTranslation } from 'react-i18next'
import { ArrowsClockwise, Check, Copy, Trash, WarningCircle } from '@phosphor-icons/react'
import { syncpointsApi } from '@/lib/api/syncpoints'
import { usersApi } from '@/lib/api/users'
import type { ApiKeyDto } from '@/lib/api/types'
import { formatDate, relativeTime } from '@/lib/utils/format'
import { Button } from '@/components/ui/Button'
import { Dialog } from '@/components/ui/Dialog'
import { IconButton } from '@/components/ui/IconButton'
import { Skeleton } from '@/components/ui/Skeleton'
import { TextField } from '@/components/ui/TextField'
import { Section } from './Section'

type ResyncTarget = { kind: 'key'; key: ApiKeyDto } | { kind: 'all' }

export function ApiKeysSection() {
  const { t } = useTranslation('account')
  const queryClient = useQueryClient()
  const keysQuery = useQuery({ queryKey: ['account', 'api-keys'], queryFn: usersApi.apiKeys })
  const activityQuery = useQuery({
    // distinct key from ActivitySection's paged query: different page size, different data
    queryKey: ['account', 'activity', 'api-key-usage'],
    queryFn: () => usersApi.authenticationActivity({ page: 0, size: 100 }),
  })
  const [comment, setComment] = useState('')
  const [created, setCreated] = useState<ApiKeyDto | null>(null)
  const [copied, setCopied] = useState(false)
  const [resyncTarget, setResyncTarget] = useState<ResyncTarget | null>(null)
  const [resyncDone, setResyncDone] = useState<string | null>(null)

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ['account', 'api-keys'] })

  // most recent successful sign-in per API key, for the "Last used" line
  const lastUsedByKey = useMemo(() => {
    const map = new Map<string, string>()
    for (const a of activityQuery.data?.content ?? []) {
      if (!a.apiKeyId || !a.success) continue
      const prev = map.get(a.apiKeyId)
      if (!prev || a.dateTime > prev) map.set(a.apiKeyId, a.dateTime)
    }
    return map
  }, [activityQuery.data])

  const createMutation = useMutation({
    mutationFn: (c: string) => usersApi.createApiKey(c),
    onSuccess: (key) => {
      setComment('')
      setCopied(false)
      setCreated(key)
      void invalidate()
    },
  })

  const deleteMutation = useMutation({
    mutationFn: (id: string) => usersApi.deleteApiKey(id),
    onSuccess: invalidate,
  })

  const resyncMutation = useMutation({
    mutationFn: (target: ResyncTarget) =>
      target.kind === 'key' ? syncpointsApi.deleteMine([target.key.id]) : syncpointsApi.deleteMine(),
    onSuccess: (_data, target) => {
      setResyncTarget(null)
      setResyncDone(target.kind === 'key' ? target.key.comment || t('apiKeys.untitled') : t('apiKeys.allDevices'))
      window.setTimeout(() => setResyncDone(null), 4000)
    },
  })

  const copy = async () => {
    if (!created?.key) return
    try {
      await navigator.clipboard.writeText(created.key)
      setCopied(true)
      window.setTimeout(() => setCopied(false), 2000)
    } catch {
      // clipboard needs a secure context; the key text stays manually selectable
    }
  }

  return (
    <Section title={t('apiKeys.title')}>
      {keysQuery.isLoading && (
        <div className="flex flex-col gap-2.5">
          <Skeleton className="h-9 w-full" />
          <Skeleton className="h-9 w-2/3" />
        </div>
      )}
      {keysQuery.isLoadingError && (
        <div className="flex items-center gap-3">
          <p className="text-sm text-danger">
            {keysQuery.error instanceof Error ? keysQuery.error.message : t('apiKeys.failed')}
          </p>
          <Button size="sm" onClick={() => void keysQuery.refetch()}>
            {t('common:action.retry')}
          </Button>
        </div>
      )}
      {keysQuery.data && (
        <>
          {keysQuery.data.length > 0 && (
            <div className="mb-1 flex items-center justify-between gap-3">
              <p className="text-xs text-ink-3">{t('apiKeys.resyncHint')}</p>
              <Button size="sm" onClick={() => setResyncTarget({ kind: 'all' })}>
                <ArrowsClockwise className="size-4" />
                {t('apiKeys.resyncAll')}
              </Button>
            </div>
          )}
          <ul className="flex flex-col divide-y divide-line">
            {keysQuery.data.map((k) => (
              <li key={k.id} className="flex items-center gap-3 py-2.5">
                <div className="min-w-0 flex-1">
                  <p className="truncate text-sm text-ink">{k.comment || t('apiKeys.untitled')}</p>
                  <p className="mt-0.5 text-xs text-ink-3">
                    {lastUsedByKey.has(k.id)
                      ? t('apiKeys.createdAndUsed', {
                          date: formatDate(k.createdDate),
                          time: relativeTime(lastUsedByKey.get(k.id)),
                        })
                      : t('apiKeys.createdOnly', { date: formatDate(k.createdDate) })}
                  </p>
                </div>
                <IconButton
                  label={t('apiKeys.resyncKey', { name: k.comment || t('apiKeys.untitled') })}
                  onClick={() => setResyncTarget({ kind: 'key', key: k })}
                >
                  <ArrowsClockwise className="size-4" />
                </IconButton>
                <IconButton
                  label={t('apiKeys.delete')}
                  onClick={() => deleteMutation.mutate(k.id)}
                  disabled={deleteMutation.isPending}
                >
                  <Trash className="size-4" />
                </IconButton>
              </li>
            ))}
            {keysQuery.data.length === 0 && <li className="py-1 text-sm text-ink-3">{t('apiKeys.empty')}</li>}
          </ul>
        </>
      )}
      {deleteMutation.isError && (
        <p className="mt-2 text-sm text-danger">
          {deleteMutation.error instanceof Error ? deleteMutation.error.message : t('apiKeys.deleteFailed')}
        </p>
      )}
      {resyncDone && <p className="mt-2 text-sm text-accent-strong">{t('apiKeys.resyncDone', { name: resyncDone })}</p>}

      <form
        className="mt-4 flex items-end gap-3"
        onSubmit={(e) => {
          e.preventDefault()
          const c = comment.trim()
          if (c) createMutation.mutate(c)
        }}
      >
        <TextField
          label={t('apiKeys.new')}
          value={comment}
          onChange={(e) => setComment(e.target.value)}
          placeholder={t('apiKeys.commentPlaceholder')}
          className="flex-1"
        />
        <Button type="submit" variant="primary" loading={createMutation.isPending} disabled={!comment.trim()}>
          {t('common:action.create')}
        </Button>
      </form>
      {createMutation.isError && (
        <p className="mt-2 text-sm text-danger">
          {createMutation.error instanceof Error ? createMutation.error.message : t('apiKeys.createFailed')}
        </p>
      )}

      <Dialog open={!!created} onOpenChange={(open) => !open && setCreated(null)} title={t('apiKeys.createdTitle')} size="sm">
        <div className="flex flex-col gap-4 p-5">
          <p className="flex items-center gap-2 text-sm font-medium text-danger">
            <WarningCircle className="size-4 shrink-0" />
            {t('apiKeys.shownOnce')}
          </p>
          <code className="rounded-lg border border-line bg-raised p-3 font-mono text-[13px] break-all text-ink select-all">
            {created?.key}
          </code>
          <div className="flex justify-end">
            <Button onClick={() => void copy()}>
              {copied ? <Check className="size-4" /> : <Copy className="size-4" />}
              {copied ? t('apiKeys.copied') : t('apiKeys.copy')}
            </Button>
          </div>
        </div>
      </Dialog>

      <Dialog
        open={!!resyncTarget}
        onOpenChange={(open) => {
          if (!open) {
            resyncMutation.reset()
            setResyncTarget(null)
          }
        }}
        title={t('apiKeys.resyncTitle')}
        size="sm"
      >
        <div className="px-5 py-4">
          <p className="text-sm text-ink-2">
            {resyncTarget?.kind === 'key' ? (
              <Trans
                i18nKey="account:apiKeys.resyncKeyBody"
                values={{ name: resyncTarget.key.comment || t('apiKeys.untitled') }}
                components={{ name: <span className="font-medium text-ink" /> }}
              />
            ) : (
              t('apiKeys.resyncAllBody')
            )}
          </p>
          {resyncMutation.isError && (
            <p className="mt-3 text-sm text-danger">
              {resyncMutation.error instanceof Error ? resyncMutation.error.message : t('apiKeys.resyncFailed')}
            </p>
          )}
        </div>
        <div className="flex justify-end gap-2 border-t border-line px-5 py-3.5">
          <Button
            variant="ghost"
            onClick={() => {
              resyncMutation.reset()
              setResyncTarget(null)
            }}
          >
            {t('common:action.cancel')}
          </Button>
          <Button
            variant="primary"
            loading={resyncMutation.isPending}
            onClick={() => resyncTarget && resyncMutation.mutate(resyncTarget)}
          >
            {t('apiKeys.resyncTitle')}
          </Button>
        </div>
      </Dialog>
    </Section>
  )
}
