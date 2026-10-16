import { useEffect, useRef, useState } from 'react'
import { Link, useSearchParams } from 'react-router-dom'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { ArrowSquareOut, ChartLineUp, PlugsConnected, SignOut, Trash } from '@phosphor-icons/react'
import { trackerEntryUrl, trackerLinkKeys, trackersApi } from '@/lib/api/trackers'
import { ApiError } from '@/lib/api/client'
import type { KomfOAuthProvider } from '@/lib/api/types'
import { urls } from '@/lib/utils/urls'
import { librariesApi } from '@/lib/api/libraries'
import { Button } from '@/components/ui/Button'
import { IconButton } from '@/components/ui/IconButton'
import { Tooltip } from '@/components/ui/Tooltip'
import { EmptyState } from '@/components/ui/EmptyState'
import { PageHeader } from '@/components/ui/PageHeader'
import { ConfirmDeleteDialog } from '@/components/detail/ConfirmDeleteDialog'
import { CoverImage } from '@/components/media/CoverImage'
import { Switch } from '@/components/ui/Switch'
import { SegmentedControl } from '@/components/ui/SegmentedControl'
import { useDocumentTitle } from '@/lib/hooks/useDocumentTitle'
import { showToast } from '@/lib/store/toast'

const PROVIDERS: KomfOAuthProvider[] = ['anilist', 'bangumi', 'mal', 'mangabaka']

/** The user's tracker hub: one login per platform plus every series binding
 * they created. Entirely per-user; admins see their own accounts here too. */
export function TrackersPage() {
  const { t } = useTranslation('trackers')
  useDocumentTitle(t('page.title'))
  const queryClient = useQueryClient()
  const [searchParams, setSearchParams] = useSearchParams()
  const [deleting, setDeleting] = useState<{
    seriesId: string
    provider: KomfOAuthProvider
  } | null>(null)

  // the OAuth callback lands here with ?oauth=...; acknowledge once and drop the query
  const oauthHandled = useRef(false)
  useEffect(() => {
    const oauth = searchParams.get('oauth')
    if (oauth === null || oauthHandled.current) return
    oauthHandled.current = true
    if (oauth === 'success') showToast(t('toast.loginSuccess'))
    else if (oauth === 'error') showToast(searchParams.get('message') || t('toast.loginError'))
    const next = new URLSearchParams(searchParams)
    next.delete('oauth')
    next.delete('message')
    setSearchParams(next, { replace: true })
    void queryClient.invalidateQueries({ queryKey: ['trackers', 'oauth'] })
  }, [searchParams, setSearchParams, queryClient, t])

  const linksQuery = useQuery({
    queryKey: trackerLinkKeys.all,
    queryFn: () => trackersApi.listLinks(),
    retry: false,
  })

  const deleteMutation = useMutation({
    mutationFn: (link: { seriesId: string; provider: KomfOAuthProvider }) =>
      trackersApi.deleteLink(link.seriesId, link.provider),
    onSuccess: () => {
      // the dialog is controlled solely by `deleting`: close it so the
      // delete action can't be repeated against an already-removed binding
      setDeleting(null)
      queryClient.invalidateQueries({ queryKey: trackerLinkKeys.all })
      showToast(t('toast.unlinked'))
    },
    onError: (e) => showToast(e instanceof Error ? e.message : t('toast.unlinkFailed')),
  })

  if (linksQuery.error instanceof ApiError && linksQuery.error.status === 409) {
    // komf not connected: point at the admin setup instead of an empty page
    return (
      <div>
        <PageHeader title={t('page.title')} />
        <EmptyState
          icon={<PlugsConnected className="size-8" />}
          title={t('page.notConnected')}
          body={t('page.notConnectedBody')}
        />
      </div>
    )
  }

  const links = linksQuery.data ?? []

  return (
    <div>
      <PageHeader title={t('page.title')} />
      <div className="mt-6 grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        {PROVIDERS.map((provider) => (
          <ProviderCard key={provider} provider={provider} />
        ))}
      </div>

      <DefaultTrackerSection />

      <VisibilitySection />

      <h2 className="mt-10 mb-3 flex items-center gap-2 font-display text-lg font-semibold text-ink">
        <ChartLineUp className="size-5 text-ink-3" />
        {t('page.bindingsHeading')}
      </h2>
      {linksQuery.isPending ? null : links.length === 0 ? (
        <p className="text-sm text-ink-3">{t('section.empty')}</p>
      ) : (
        <ul className="divide-y divide-line rounded-xl border border-line bg-surface">
          {links.map((link) => (
            <li key={`${link.seriesId}-${link.provider}`} className="flex items-center gap-3 px-4 py-3">
              <CoverImage src={urls.seriesThumbnail(link.seriesId)} alt={link.title ?? link.trackId} className="w-11 shrink-0" />
              <div className="min-w-0 flex-1">
                <Link
                  to={`/series/${link.seriesId}`}
                  className="truncate text-sm font-medium text-ink transition-colors hover:text-accent-strong"
                >
                  {link.title || `#${link.trackId}`}
                </Link>
                <p className="mt-0.5 flex flex-wrap items-center gap-x-1.5 text-xs text-ink-3">
                  <span className="rounded bg-raised px-1.5 py-0.5 text-[11px] font-semibold text-ink-2">
                    {t(`provider.${link.provider}`)}
                  </span>
                  <a
                    href={trackerEntryUrl(link.provider, link.trackId)}
                    target="_blank"
                    rel="noreferrer"
                    className="inline-flex items-center gap-0.5 font-mono transition-colors hover:text-accent-strong"
                  >
                    #{link.trackId}
                    <ArrowSquareOut className="size-3" />
                  </a>
                  <span aria-hidden>·</span>
                  <span>{t(`mode.${link.trackMode}`)}</span>
                  {link.chapterOffset !== 0 && (
                    <>
                      <span aria-hidden>·</span>
                      <span>{t('page.offset', { offset: link.chapterOffset })}</span>
                    </>
                  )}
                </p>
              </div>
              <Tooltip content={t('row.unlink')}>
                <IconButton label={t('row.unlink')} onClick={() => setDeleting(link)}>
                  <Trash className="size-4" />
                </IconButton>
              </Tooltip>
            </li>
          ))}
        </ul>
      )}

      <ConfirmDeleteDialog
        open={!!deleting}
        onOpenChange={(open) => !open && setDeleting(null)}
        title={t('unlinkDialog.title')}
        name={deleting ? t('unlinkDialog.name', { provider: t(`provider.${deleting.provider}`) }) : ''}
        loading={deleteMutation.isPending}
        onConfirm={() => deleting && deleteMutation.mutate(deleting)}
      />
    </div>
  )
}

function VisibilitySection() {
  const { t } = useTranslation('trackers')
  const queryClient = useQueryClient()
  const librariesQuery = useQuery({ queryKey: ['libraries'], queryFn: librariesApi.list })
  const prefsQuery = useQuery({
    queryKey: trackerLinkKeys.preferences,
    queryFn: () => trackersApi.getPreferences(),
    retry: false,
  })
  const saveMutation = useMutation({
    mutationFn: (libraries: string[]) =>
      trackersApi.putPreferences({ libraries, defaultTracker: prefsQuery.data?.defaultTracker }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: trackerLinkKeys.preferences }),
    onError: (e) => showToast(e instanceof Error ? e.message : t('toast.visibilityFailed')),
  })
  const selected = new Set(prefsQuery.data?.libraries ?? [])
  const allIds = (librariesQuery.data ?? []).map((l) => l.id)
  const handleChange = (libraryId: string, nextChecked: boolean) => {
    // the PUT replaces the whole preferences object, so toggling before the
    // current values load would reset defaultTracker to empty
    if (!prefsQuery.data) return
    // empty list = unrestricted; the first explicit toggle materializes the
    // restriction list (turning one library off excludes just that one)
    const next =
      selected.size === 0
        ? nextChecked
          ? allIds
          : allIds.filter((id) => id !== libraryId)
        : nextChecked
          ? [...selected, libraryId]
          : [...selected].filter((id) => id !== libraryId)
    saveMutation.mutate(next)
  }
  return (
    <section className="mt-10">
      <h2 className="mb-1 font-display text-lg font-semibold text-ink">{t('page.visibilityHeading')}</h2>
      <p className="mb-3 text-xs text-ink-3">{t('page.visibilityHint')}</p>
      <ul className="divide-y divide-line rounded-xl border border-line bg-surface">
        {(librariesQuery.data ?? []).map((library) => (
          <li key={library.id} className="flex items-center justify-between gap-3 px-4 py-2.5">
            <span className="min-w-0 truncate text-sm text-ink">{library.name}</span>
            <Switch
              checked={selected.size === 0 || selected.has(library.id)}
              onCheckedChange={(on) => handleChange(library.id, on)}
              disabled={saveMutation.isPending || !prefsQuery.data}
              label={library.name}
            />
          </li>
        ))}
      </ul>
    </section>
  )
}

function DefaultTrackerSection() {
  const { t } = useTranslation('trackers')
  const queryClient = useQueryClient()
  const prefsQuery = useQuery({
    queryKey: trackerLinkKeys.preferences,
    queryFn: () => trackersApi.getPreferences(),
    retry: false,
  })
  const saveMutation = useMutation({
    mutationFn: (defaultTracker: KomfOAuthProvider | undefined) =>
      trackersApi.putPreferences({
        libraries: prefsQuery.data?.libraries ?? [],
        defaultTracker,
      }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: trackerLinkKeys.preferences }),
    onError: (e) => showToast(e instanceof Error ? e.message : t('toast.visibilityFailed')),
  })
  return (
    <section className="mt-10">
      <h2 className="mb-1 font-display text-lg font-semibold text-ink">{t('page.defaultHeading')}</h2>
      <p className="mb-3 text-xs text-ink-3">{t('page.defaultHint')}</p>
      <SegmentedControl
        value={prefsQuery.data?.defaultTracker ?? 'none'}
        onChange={(v) => {
          // the PUT replaces the whole preferences object, so switching before
          // the current values load would reset libraries to empty
          if (!prefsQuery.data) return
          saveMutation.mutate(v === 'none' ? undefined : (v as KomfOAuthProvider))
        }}
        options={[
          { value: 'none', label: t('page.defaultNone') },
          ...PROVIDERS.map((p) => ({ value: p, label: t(`provider.${p}`) })),
        ]}
      />
    </section>
  )
}

function ProviderCard({ provider }: { provider: KomfOAuthProvider }) {
  const { t } = useTranslation('trackers')
  const queryClient = useQueryClient()
  const statusQuery = useQuery({
    queryKey: ['trackers', 'oauth', provider],
    queryFn: () => trackersApi.oauthStatus(provider),
    retry: false,
  })
  const logoutMutation = useMutation({
    mutationFn: () => trackersApi.oauthLogout(provider),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['trackers', 'oauth', provider] })
      queryClient.invalidateQueries({ queryKey: ['trackers', 'state'] })
      showToast(t('toast.loggedOut', { provider: t(`provider.${provider}`) }))
    },
    onError: (e) => showToast(e instanceof Error ? e.message : t('toast.logoutFailed')),
  })
  const loggedIn = statusQuery.data?.logged_in ?? false
  return (
    <div className="flex flex-col gap-3 rounded-xl border border-line bg-surface p-4">
      <div className="flex items-center justify-between">
        <p className="text-sm font-semibold text-ink">{t(`provider.${provider}`)}</p>
        {statusQuery.isPending ? (
          <p className="text-xs text-ink-3">{t('state.loading')}</p>
        ) : loggedIn ? (
          <p className="truncate text-xs text-ink-3">{statusQuery.data?.username}</p>
        ) : (
          <p className="text-xs font-medium text-danger">{t('page.notLoggedIn')}</p>
        )}
      </div>
      {loggedIn ? (
        <Button variant="secondary" size="sm" loading={logoutMutation.isPending} onClick={() => logoutMutation.mutate()}>
          <SignOut className="size-4" />
          {t('page.logout')}
        </Button>
      ) : (
        <Button
          variant="secondary"
          size="sm"
          // full browser navigation: the endpoint 302s into the OAuth flow
          onClick={() => window.location.assign(trackersApi.oauthStartUrl(provider))}
        >
          <ArrowSquareOut className="size-4" />
          {t('page.login')}
        </Button>
      )}
    </div>
  )
}
