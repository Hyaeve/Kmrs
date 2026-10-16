import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { BookOpen, Plus } from '@phosphor-icons/react'
import { trackerLinkKeys, trackersApi, trackerUrlsFromMetadata } from '@/lib/api/trackers'
import { ApiError } from '@/lib/api/client'
import type { KomfOAuthProvider, TrackerLink, TrackerLinkRef, TrackerState } from '@/lib/api/types'
import { Button } from '@/components/ui/Button'
import { Dialog } from '@/components/ui/Dialog'
import { TextField } from '@/components/ui/TextField'
import { SegmentedControl } from '@/components/ui/SegmentedControl'
import { CoverImage } from '@/components/media/CoverImage'
import { showToast } from '@/lib/store/toast'

const PROVIDERS: KomfOAuthProvider[] = ['anilist', 'bangumi', 'mal', 'mangabaka']

const STATUS_KEYS = ['reading', 'planning', 'completed', 'paused', 'dropped', 'rereading'] as const
type TrackerStatusKey = (typeof STATUS_KEYS)[number]

function isNotConnected(error: unknown): boolean {
  return error instanceof ApiError && error.status === 409
}

/** Compact tracking pills on the series detail page: one pill per binding plus
 * an add button; everything else lives in the dialogs the pills open. Hidden
 * entirely when the komf integration is not connected (409). */
export function TrackerSection({
  seriesId,
  seriesTitle,
  libraryId,
  metadataLinks,
}: {
  seriesId: string
  seriesTitle: string
  libraryId: string
  metadataLinks: Array<{ url: string }>
}) {
  const { t } = useTranslation('trackers')
  const queryClient = useQueryClient()
  const [bindOpen, setBindOpen] = useState(false)
  const [editing, setEditing] = useState<TrackerLinkRef | null>(null)

  const linksQuery = useQuery({
    queryKey: trackerLinkKeys.series(seriesId),
    queryFn: () => trackersApi.listSeriesLinks(seriesId),
    retry: false,
  })
  const preferencesQuery = useQuery({
    queryKey: trackerLinkKeys.preferences,
    queryFn: () => trackersApi.getPreferences(),
    retry: false,
  })
  const links = linksQuery.data ?? []

  const deleteMutation = useMutation({
    mutationFn: (link: TrackerLinkRef) => trackersApi.deleteLink(seriesId, link.provider),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: trackerLinkKeys.all })
      queryClient.invalidateQueries({ queryKey: trackerLinkKeys.series(seriesId) })
      showToast(t('toast.unlinked'))
    },
    onError: (e) => showToast(e instanceof Error ? e.message : t('toast.unlinkFailed')),
  })

  if (isNotConnected(linksQuery.error)) return null
  // a configured library list restricts where the module shows; empty = everywhere
  const restrictedTo = preferencesQuery.data?.libraries
  if (restrictedTo && restrictedTo.length > 0 && !restrictedTo.includes(libraryId)) return null

  return (
    <div className="mt-4 flex flex-wrap items-center gap-2">
      {links.map((link) => (
        <TrackerPill key={link.provider} link={link} onEdit={() => setEditing(link)} />
      ))}
      <Button variant="ghost" size="sm" onClick={() => setBindOpen(true)}>
        <Plus className="size-4" />
        {t('section.add')}
      </Button>

      <BindDialog
        // remounting on open resets provider/query to the first free provider
        key={String(bindOpen)}
        open={bindOpen}
        onOpenChange={setBindOpen}
        seriesId={seriesId}
        seriesTitle={seriesTitle}
        existingProviders={links.map((l) => l.provider)}
        suggestedUrls={trackerUrlsFromMetadata(metadataLinks)}
        defaultProvider={preferencesQuery.data?.defaultTracker}
        // a fresh binding opens the options dialog instead of just closing:
        // mode/offset/status are almost always worth setting right away
        onBound={setEditing}
      />
      {editing && (
        <EditDialog
          link={editing}
          onClose={() => setEditing(null)}
          onUnlink={() => {
            setEditing(null)
            deleteMutation.mutate(editing)
          }}
        />
      )}
    </div>
  )
}

function TrackerPill({ link, onEdit }: { link: TrackerLink; onEdit: () => void }) {
  const { t } = useTranslation('trackers')
  const stateQuery = useQuery({
    queryKey: ['trackers', 'state', link.provider, link.trackId],
    queryFn: () => trackersApi.getState(link.provider, link.trackId),
    retry: false,
    // 401 (not logged in) is a normal state: the pill still shows, dotted
  })
  const unauthorized =
    stateQuery.error instanceof ApiError && stateQuery.error.status === 401
  const status = stateQuery.data?.status
  return (
    <button
      type="button"
      onClick={onEdit}
      className="inline-flex cursor-pointer items-center gap-1.5 rounded-full border border-line bg-raised px-3 py-1.5 text-xs font-medium text-ink transition-colors hover:border-line-strong hover:bg-overlay"
    >
      {t(`provider.${link.provider}`)}
      {stateQuery.isPending ? null : unauthorized ? (
        <span className="size-1.5 rounded-full bg-danger" aria-label={t('state.loginRequired')} />
      ) : (
        status && <span className="text-ink-3">{t(`status.${status}`)}</span>
      )}
    </button>
  )
}

function BindDialog({
  open,
  onOpenChange,
  seriesId,
  seriesTitle,
  existingProviders,
  suggestedUrls,
  defaultProvider,
  onBound,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  seriesId: string
  seriesTitle: string
  existingProviders: KomfOAuthProvider[]
  /** platform entry URLs recognized in the series metadata links */
  suggestedUrls: Partial<Record<KomfOAuthProvider, string>>
  /** preselected provider from the management page; falls back to the first free one */
  defaultProvider?: KomfOAuthProvider
  /** receives the freshly created binding so the caller can open its options */
  onBound: (link: TrackerLinkRef) => void
}) {
  const { t } = useTranslation('trackers')
  const queryClient = useQueryClient()
  const available = PROVIDERS.filter((p) => !existingProviders.includes(p))
  // remount-on-open (key at the call site) makes these initial values per-open
  const [provider, setProvider] = useState<KomfOAuthProvider>(
    (defaultProvider && available.includes(defaultProvider) ? defaultProvider : available[0]) ?? 'anilist',
  )
  const [query, setQuery] = useState(seriesTitle)

  const searchQuery = useQuery({
    queryKey: ['trackers', 'search', provider, query],
    queryFn: () => trackersApi.search(provider, { name: query }),
    enabled: open && query.trim().length > 0,
    retry: false,
  })

  // a series-metadata link for the selected provider resolves through komf's
  // search box exactly like a pasted link: one direct, clickable candidate
  const suggestedUrl = suggestedUrls[provider]
  const suggestQuery = useQuery({
    queryKey: ['trackers', 'search', provider, suggestedUrl],
    queryFn: () => trackersApi.search(provider, { name: suggestedUrl! }),
    enabled: open && !!suggestedUrl,
    retry: false,
  })

  const bindMutation = useMutation({
    mutationFn: (item: { id: string; title: string }) =>
      trackersApi.upsertLink(seriesId, { provider, trackId: item.id, title: item.title }),
    onSuccess: (bound) => {
      queryClient.invalidateQueries({ queryKey: trackerLinkKeys.all })
      queryClient.invalidateQueries({ queryKey: trackerLinkKeys.series(seriesId) })
      onOpenChange(false)
      onBound(bound)
      showToast(t('toast.linked'))
    },
    onError: (e) => showToast(e instanceof Error ? e.message : t('toast.linkFailed')),
  })

  return (
    <Dialog open={open} onOpenChange={onOpenChange} title={t('bindDialog.title')} size="lg">
      <div className="flex flex-col gap-4 p-5">
        <SegmentedControl
          options={PROVIDERS.map((p) => ({ value: p, label: t(`provider.${p}`) }))}
          value={provider}
          onChange={setProvider}
        />
        <TextField
          label={t('bindDialog.searchLabel')}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={t('bindDialog.searchPlaceholder')}
        />
        <div className="min-h-40 flex-1 overflow-y-auto">
          {suggestedUrl && suggestQuery.data?.[0] && (
            <div className="mb-2">
              <p className="mb-1 text-[11px] font-medium tracking-wide text-ink-3 uppercase">
                {t('bindDialog.fromLinks')}
              </p>
              <ul className="divide-y divide-line rounded-lg border border-accent/30 bg-accent/5 px-3">
                <ResultRow
                  item={suggestQuery.data[0]}
                  binding={bindMutation.isPending}
                  onBind={() => bindMutation.mutate(suggestQuery.data![0])}
                />
              </ul>
            </div>
          )}
          {searchQuery.isPending ? (
            <p className="py-6 text-center text-sm text-ink-3">{t('state.loading')}</p>
          ) : searchQuery.isError ? (
            <p className="py-6 text-center text-sm text-danger">{searchQuery.error.message}</p>
          ) : (searchQuery.data?.length ?? 0) === 0 ? (
            <p className="py-6 text-center text-sm text-ink-3">{t('bindDialog.noResults')}</p>
          ) : (
            <ul className="divide-y divide-line">
              {searchQuery.data!.map((item) => (
                <ResultRow key={item.id} item={item} binding={bindMutation.isPending} onBind={() => bindMutation.mutate(item)} />
              ))}
            </ul>
          )}
        </div>
      </div>
    </Dialog>
  )
}

function ResultRow({
  item,
  binding,
  onBind,
}: {
  item: { id: string; title: string; coverUrl?: string; tracked: boolean; mediaType?: 'manga' | 'novel' }
  binding: boolean
  onBind: () => void
}) {
  const { t } = useTranslation('trackers')
  return (
    <li className="flex items-center gap-3 py-2">
      {item.coverUrl ? (
        <CoverImage src={item.coverUrl} alt={item.title} className="w-11 shrink-0" referrerPolicy="no-referrer" />
      ) : (
        <div className="cover-aspect flex w-11 shrink-0 items-center justify-center rounded-lg bg-raised text-ink-3">
          <BookOpen className="size-5" weight="duotone" />
        </div>
      )}
      <div className="min-w-0 flex-1">
        <p className="truncate text-sm font-medium text-ink">{item.title}</p>
        {(item.mediaType || item.tracked) && (
          <p className="mt-0.5 flex flex-wrap items-center gap-x-1.5 text-xs">
            {item.mediaType && (
              <span className="rounded bg-raised px-1.5 py-0.5 text-[11px] font-medium text-ink-3">
                {t(`mediaType.${item.mediaType}`)}
              </span>
            )}
            {item.tracked && (
              <span className="font-medium text-accent-strong">{t('bindDialog.tracked')}</span>
            )}
          </p>
        )}
      </div>
      <Button variant="secondary" size="sm" loading={binding} onClick={onBind}>
        {t('bindDialog.bind')}
      </Button>
    </li>
  )
}

function EditDialog({
  link,
  onClose,
  onUnlink,
}: {
  link: TrackerLinkRef
  onClose: () => void
  onUnlink: () => void
}) {
  const { t } = useTranslation('trackers')
  // the pill query usually warmed this cache; refetching here is cheap and the
  // form only renders once the current remote values are known
  const stateQuery = useQuery({
    queryKey: ['trackers', 'state', link.provider, link.trackId],
    queryFn: () => trackersApi.getState(link.provider, link.trackId),
    retry: false,
  })

  if (stateQuery.isPending) {
    return (
      <Dialog open onOpenChange={(open) => !open && onClose()} title={t('editDialog.title')} size="sm">
        <p className="p-5 text-sm text-ink-3">{t('state.loading')}</p>
      </Dialog>
    )
  }
  if (stateQuery.isError) {
    return (
      <Dialog open onOpenChange={(open) => !open && onClose()} title={t('editDialog.title')} size="sm">
        <p className="p-5 text-sm text-danger">{stateQuery.error.message}</p>
      </Dialog>
    )
  }
  return <EditDialogForm link={link} state={stateQuery.data} onClose={onClose} onUnlink={onUnlink} />
}

function EditDialogForm({
  link,
  state,
  onClose,
  onUnlink,
}: {
  link: TrackerLinkRef
  state?: TrackerState
  onClose: () => void
  onUnlink: () => void
}) {
  const { t } = useTranslation('trackers')
  const queryClient = useQueryClient()
  const [mode, setMode] = useState(link.trackMode)
  const [offset, setOffset] = useState(String(link.chapterOffset))
  // pre-filled from the platform entry: what you see is what is stored there
  const remoteStatus = state?.status
  const [status, setStatus] = useState<TrackerStatusKey>(
    remoteStatus && (STATUS_KEYS as readonly string[]).includes(remoteStatus)
      ? (remoteStatus as TrackerStatusKey)
      : 'reading',
  )
  const [score, setScore] = useState(state?.score != null ? String(state.score) : '')
  const [chapter, setChapter] = useState(state?.lastReadChapter != null ? String(state.lastReadChapter) : '')
  const [volume, setVolume] = useState(state?.lastReadVolume != null ? String(state.lastReadVolume) : '')

  const saveMutation = useMutation({
    mutationFn: async () => {
      // binding options always go to kmrs
      await trackersApi.upsertLink(link.seriesId, {
        provider: link.provider,
        trackId: link.trackId,
        title: link.title,
        trackMode: mode,
        chapterOffset: Number(offset) || 0,
      })
      // remote fields go to the platform; empty fields are left untouched
      const payload: Parameters<typeof trackersApi.update>[1] = { trackId: link.trackId, status }
      if (score.trim() !== '') payload.score = Number(score)
      if (chapter.trim() !== '') payload.lastReadChapter = Number(chapter)
      if (volume.trim() !== '') payload.lastReadVolume = Number(volume)
      await trackersApi.update(link.provider, payload)
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: trackerLinkKeys.all })
      queryClient.invalidateQueries({ queryKey: trackerLinkKeys.series(link.seriesId) })
      queryClient.invalidateQueries({ queryKey: ['trackers', 'state'] })
      onClose()
      showToast(t('toast.saved'))
    },
    onError: (e) => showToast(e instanceof Error ? e.message : t('toast.saveFailed')),
  })

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()} title={t('editDialog.title')} size="sm">
      <div className="flex flex-col gap-4 p-5">
        <SegmentedControl
          options={(['auto', 'chapter', 'volume'] as const).map((m) => ({
            value: m,
            label: t(`mode.${m}`),
          }))}
          value={mode}
          onChange={setMode}
        />
        <TextField
          label={t('editDialog.chapterOffset')}
          type="number"
          value={offset}
          onChange={(e) => setOffset(e.target.value)}
          helper={t('editDialog.chapterOffsetHelper')}
        />
        <div className="border-t border-line pt-4">
          <p className="mb-3 text-[13px] font-medium text-ink-2">{t('editDialog.remoteHeading')}</p>
          <div className="flex flex-col gap-3">
            <SegmentedControl
              size="sm"
              options={STATUS_KEYS.map((s) => ({ value: s, label: t(`status.${s}`) }))}
              value={status}
              onChange={setStatus}
            />
            <div className="grid grid-cols-3 gap-3">
              <TextField label={t('editDialog.score')} type="number" value={score} onChange={(e) => setScore(e.target.value)} placeholder="—" />
              <TextField label={t('editDialog.chapter')} type="number" value={chapter} onChange={(e) => setChapter(e.target.value)} placeholder="—" />
              <TextField label={t('editDialog.volume')} type="number" value={volume} onChange={(e) => setVolume(e.target.value)} placeholder="—" />
            </div>
          </div>
        </div>
        <div className="flex items-center justify-between gap-2">
          <Button variant="danger" size="sm" onClick={onUnlink}>
            {t('row.unlink')}
          </Button>
          <div className="flex gap-2">
            <Button variant="secondary" onClick={onClose}>
              {t('common:action.cancel')}
            </Button>
            <Button variant="primary" loading={saveMutation.isPending} onClick={() => saveMutation.mutate()}>
              {t('common:action.save')}
            </Button>
          </div>
        </div>
      </div>
    </Dialog>
  )
}
