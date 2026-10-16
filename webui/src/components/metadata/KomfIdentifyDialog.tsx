import { useEffect, useMemo, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { keepPreviousData, useMutation, useQuery } from '@tanstack/react-query'
import { BookOpen, CircleNotch, Link as LinkIcon, MagnifyingGlass, Stack } from '@phosphor-icons/react'
import { komfApi } from '@/lib/api/komf'
import { seriesApi } from '@/lib/api/series'
import type { SeriesDto, WebLinkDto } from '@/lib/api/types'
import { cn } from '@/lib/utils/cn'
import { trackKomfJob } from '@/lib/store/komfJobs'
import { Dialog } from '@/components/ui/Dialog'
import { Button } from '@/components/ui/Button'
import { Skeleton } from '@/components/ui/Skeleton'
import { EmptyState } from '@/components/ui/EmptyState'
import { CoverImage } from '@/components/media/CoverImage'

interface KomfIdentifyDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  series: SeriesDto
  onIdentified: () => void
}

export function KomfIdentifyDialog({ open, onOpenChange, series, onIdentified }: KomfIdentifyDialogProps) {
  const { t } = useTranslation('metadata')
  return (
    <Dialog open={open} onOpenChange={onOpenChange} title={t('detail:menu.identifyKomf')}>
      {/* Dialog unmounts its children on close, so the search and mutation state
          restart fresh on every open */}
      <IdentifyContent series={series} onIdentified={onIdentified} close={() => onOpenChange(false)} />
    </Dialog>
  )
}

interface ProviderLinkHit {
  provider: string
  providerSeriesId: string
  url: string
}

const PROVIDER_LINK_LABELS: Record<string, string> = {
  BANGUMI: 'Bangumi',
  EHENTAI: 'E-Hentai',
  ANILIST: 'AniList',
  MAL: 'MAL',
  MANGADEX: 'MangaDex',
  MANGA_UPDATES: 'MangaUpdates',
}

/** Recognizes provider URLs in series/book links; ids follow komf's SCREAMING_SNAKE names. */
function parseProviderLink(link: WebLinkDto): ProviderLinkHit | null {
  const url = link.url.toLowerCase()
  if ((url.includes('bgm.tv') || url.includes('bangumi.tv')) && url.includes('/subject/')) {
    const m = url.match(/\/subject\/([^/?]+)/)
    if (m) return { provider: 'BANGUMI', providerSeriesId: m[1], url: link.url }
  }
  if (url.includes('e-hentai.org') || url.includes('exhentai.org')) {
    const m = url.match(/\/g\/([^/]+)\/([^/?]+)/)
    if (m) return { provider: 'EHENTAI', providerSeriesId: `${m[1]};${m[2]}`, url: link.url }
  }
  if (url.includes('anilist.co')) {
    const m = url.match(/\/(?:anime|manga)\/(\d+)/)
    if (m) return { provider: 'ANILIST', providerSeriesId: m[1], url: link.url }
  }
  if (url.includes('myanimelist.net')) {
    const m = url.match(/\/(?:anime|manga)\/(\d+)/)
    if (m) return { provider: 'MAL', providerSeriesId: m[1], url: link.url }
  }
  if (url.includes('mangadex.org')) {
    const m = url.match(/\/title\/([^/?]+)/)
    if (m) return { provider: 'MANGADEX', providerSeriesId: m[1], url: link.url }
  }
  if (url.includes('mangaupdates.com')) {
    const m = url.match(/\/series\/([^/?]+)/) || url.match(/series\.html\?id=(\d+)/)
    if (m) return { provider: 'MANGA_UPDATES', providerSeriesId: m[1], url: link.url }
  }
  return null
}

function IdentifyContent({
  series,
  onIdentified,
  close,
}: {
  series: SeriesDto
  onIdentified: () => void
  close: () => void
}) {
  const { t } = useTranslation('metadata')
  const [text, setText] = useState(series.metadata.title || series.name)
  const [search, setSearch] = useState(text)

  useEffect(() => {
    const t = setTimeout(() => setSearch(text.trim()), 300)
    return () => clearTimeout(t)
  }, [text])

  const q = useQuery({
    queryKey: ['komf-search', series.id, search],
    queryFn: () => komfApi.search({ name: search, libraryId: series.libraryId, seriesId: series.id }),
    enabled: search.length > 0,
    placeholderData: keepPreviousData,
  })

  // oneshots carry their provider links on the single book when the series has none
  const needBookLinks = series.oneshot && series.metadata.links.length === 0
  const bookQuery = useQuery({
    queryKey: ['series', series.id, 'oneshot-book'],
    queryFn: async () => (await seriesApi.books(series.id, { size: 1 })).content[0] ?? null,
    enabled: needBookLinks,
  })
  const bookLinks = bookQuery.data?.metadata.links
  const linkHits = useMemo(() => {
    const all = series.metadata.links.length > 0 ? series.metadata.links : (bookLinks ?? [])
    const seen = new Set<string>()
    return all
      .map(parseProviderLink)
      .filter((h): h is ProviderLinkHit => h !== null)
      .filter((h) => {
        const key = `${h.provider}:${h.providerSeriesId}`
        return !seen.has(key) && (seen.add(key), true)
      })
  }, [series.metadata.links, bookLinks])

  const identify = useMutation({
    mutationFn: (r: { provider: string; providerSeriesId: string }) =>
      komfApi.identify({
        libraryId: series.libraryId,
        seriesId: series.id,
        provider: r.provider,
        providerSeriesId: r.providerSeriesId,
      }),
    onSuccess: (data) => {
      trackKomfJob(data.id, series.metadata.title || series.name)
      close()
      onIdentified()
    },
  })

  const results = q.data ?? []
  const pendingKey =
    identify.isPending && identify.variables ? `${identify.variables.provider}:${identify.variables.providerSeriesId}` : null

  const linkRow = (pendingId: string, onClick: () => void, icon: ReactNode, title: string, caption: string, key?: string) => (
    <li key={key ?? pendingId}>
      <button
        type="button"
        disabled={identify.isPending}
        onClick={onClick}
        className="flex w-full cursor-pointer items-center gap-3 rounded-lg px-2 py-2 text-left transition-colors hover:bg-raised disabled:cursor-default disabled:opacity-60"
      >
        <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-raised text-ink-3">{icon}</span>
        <span className="min-w-0 flex-1">
          <span className="block truncate text-sm font-medium text-ink">{title}</span>
          <span className="mt-0.5 block truncate font-mono text-xs text-ink-3">{caption}</span>
        </span>
        {pendingKey === pendingId && <CircleNotch className="size-4 shrink-0 animate-spin text-ink-3" />}
      </button>
    </li>
  )

  return (
    <>
      <div className="px-5 pt-4">
        <div className="relative">
          <MagnifyingGlass className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-ink-3" />
          <input
            type="search"
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder={t('identify.searchPlaceholder')}
            autoFocus
            className="h-9 w-full rounded-lg border border-line bg-surface pr-3 pl-9 text-base text-ink transition-colors placeholder:text-ink-3 focus:border-accent/70 focus:outline-none"
          />
        </div>
      </div>
      {linkHits.length > 0 && (
        <div className="px-5 pt-3">
          <div className="rounded-lg border border-line">
            <p className="border-b border-line px-3 py-2 text-xs font-medium text-ink-3">{t('identify.providerLinks')}</p>
            <ul className="flex flex-col p-1">
              {linkHits.length > 1 &&
                linkRow(
                  `${linkHits[0].provider}:${linkHits[0].providerSeriesId}`,
                  () => identify.mutate(linkHits[0]),
                  <Stack className="size-4" />,
                  t('identify.aggregateAll'),
                  t('identify.aggregateHint', { count: linkHits.length }),
                  'aggregate',
                )}
              {linkHits.map((hit) =>
                linkRow(
                  `${hit.provider}:${hit.providerSeriesId}`,
                  () => identify.mutate(hit),
                  <LinkIcon className="size-4" />,
                  PROVIDER_LINK_LABELS[hit.provider] ?? hit.provider,
                  hit.providerSeriesId,
                ),
              )}
            </ul>
          </div>
        </div>
      )}
      <div className="min-h-72 px-5 py-4">
        {!search ? (
          <EmptyState title={t('identify.typeToSearch')} />
        ) : q.isPending ? (
          <div className="flex flex-col gap-2">
            {Array.from({ length: 5 }, (_, i) => (
              <div key={i} className="flex items-center gap-3 px-2 py-2">
                <Skeleton className="cover-aspect w-11 shrink-0" />
                <div className="flex min-w-0 flex-1 flex-col gap-1.5">
                  <Skeleton className="h-3.5 w-3/5" />
                  <Skeleton className="h-3 w-2/5" />
                </div>
              </div>
            ))}
          </div>
        ) : q.isLoadingError ? (
          <EmptyState
            title={t('identify.searchFailed')}
            body={q.error.message}
            action={
              <Button variant="secondary" onClick={() => q.refetch()}>
                {t('common:action.retry')}
              </Button>
            }
          />
        ) : results.length === 0 ? (
          <EmptyState title={t('identify.noResults')} body={t('identify.noResultsHint')} />
        ) : (
          <ul className="flex flex-col">
            {results.map((r) => {
              const key = `${r.provider}:${r.resultId}`
              const meta = [r.mediaType, r.language].filter(Boolean).join(' · ')
              const chipClass = 'shrink-0 rounded-full border border-line bg-raised px-2 py-0.5 text-[11px] font-medium text-ink-2'
              return (
                <li
                  key={key}
                  className={cn(
                    'flex items-center gap-2 rounded-lg px-2 py-2 transition-colors hover:bg-raised',
                    identify.isPending && 'opacity-60',
                  )}
                >
                  <button
                    type="button"
                    disabled={identify.isPending}
                    onClick={() => identify.mutate({ provider: r.provider, providerSeriesId: r.resultId })}
                    className="flex min-w-0 flex-1 cursor-pointer items-center gap-3 text-left disabled:cursor-default"
                  >
                    {r.imageUrl ? (
                      <CoverImage src={r.imageUrl} alt={r.title} className="w-11 shrink-0" referrerPolicy="no-referrer" />
                    ) : (
                      <div className="cover-aspect flex w-11 shrink-0 items-center justify-center rounded-lg bg-raised text-ink-3">
                        <BookOpen className="size-5" weight="duotone" />
                      </div>
                    )}
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-sm font-medium text-ink">{r.title}</span>
                      {meta && <span className="mt-0.5 block truncate text-xs text-ink-3">{meta}</span>}
                    </span>
                  </button>
                  {pendingKey === key ? (
                    <CircleNotch className="size-4 shrink-0 animate-spin text-ink-3" />
                  ) : r.url ? (
                    <a
                      href={r.url}
                      target="_blank"
                      rel="noreferrer"
                      className={cn(chipClass, 'transition-colors hover:border-accent/60 hover:text-accent')}
                    >
                      {r.provider}
                    </a>
                  ) : (
                    <span className={chipClass}>{r.provider}</span>
                  )}
                </li>
              )
            })}
          </ul>
        )}
      </div>
      {identify.isError && (
        <div className="border-t border-line px-5 py-3">
          <p className="text-sm text-danger">
            {identify.error instanceof Error ? identify.error.message : t('identify.queueFailed')}
          </p>
        </div>
      )}
    </>
  )
}
