import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { ArrowSquareOut, GitCommit, Rocket, Tag, WarningCircle } from '@phosphor-icons/react'
import { releasesApi } from '@/lib/api/releases'
import { serverApi } from '@/lib/api/users'
import type { ReleaseDto } from '@/lib/api/types'
import { cn } from '@/lib/utils/cn'
import { formatDate } from '@/lib/utils/format'
import { useDocumentTitle } from '@/lib/hooks/useDocumentTitle'
import { Button } from '@/components/ui/Button'
import { Chip } from '@/components/ui/Chip'
import { EmptyState } from '@/components/ui/EmptyState'
import { MarkdownContent } from '@/components/ui/Markdown'
import { PageHeader } from '@/components/ui/PageHeader'
import { Skeleton } from '@/components/ui/Skeleton'

function Badge({ children, tone }: { children: string; tone: 'current' | 'pre' }) {
  return (
    <span
      className={cn(
        'inline-flex items-center rounded-full border px-2 py-0.5 text-[11px] font-medium',
        tone === 'current' ? 'border-accent/40 bg-accent-soft text-accent-strong' : 'border-line bg-raised text-ink-2',
      )}
    >
      {children}
    </span>
  )
}

// GitHub-generated notes end every entry with a PR/compare URL; the card header
// already links to the release, and the bare URLs overflow narrow screens.
function stripLinks(markdown: string): string {
  return markdown
    .replace(/^\s*(?:\*\*)?Full Changelog(?:\*\*)?\s*:.*$/gim, '')
    .replace(/\s+in\s+https?:\/\/\S+/g, '')
    .replace(/\[([^\]]*)\]\(https?:\/\/[^)]+\)/g, '$1')
    .replace(/https?:\/\/\S+/g, '')
}

function ReleaseCard({ release, isCurrent }: { release: ReleaseDto; isCurrent: boolean }) {
  const { t, i18n } = useTranslation('admin-maintenance')
  return (
    <article className={cn('rounded-xl border bg-surface p-5', isCurrent ? 'border-accent/40' : 'border-line')}>
      <header className="flex flex-wrap items-center gap-x-3 gap-y-1.5">
        <h2 className="flex items-center gap-2 text-[15px] font-semibold text-ink">
          <Tag className="size-4 text-ink-3" />
          {release.version}
        </h2>
        {isCurrent && <Badge tone="current">{t('updates.current')}</Badge>}
        {release.preRelease && <Badge tone="pre">{t('updates.preRelease')}</Badge>}
        <time className="text-xs text-ink-3" dateTime={release.releaseDate} title={new Date(release.releaseDate).toLocaleString(i18n.language)}>
          {formatDate(release.releaseDate)}
        </time>
        <a
          href={release.url}
          target="_blank"
          rel="noreferrer"
          className="ml-auto inline-flex items-center gap-1 text-xs text-ink-3 hover:text-accent-strong"
        >
          <ArrowSquareOut className="size-3.5" />
          GitHub
        </a>
      </header>
      {release.description && <MarkdownContent markdown={stripLinks(release.description)} className="mt-3" />}
    </article>
  )
}

export function AdminUpdatesPage() {
  const { t } = useTranslation('admin-maintenance')
  const infoQuery = useQuery({ queryKey: ['server-info'], queryFn: serverApi.info, staleTime: Infinity })
  const q = useQuery({ queryKey: ['admin', 'releases'], queryFn: releasesApi.list })

  useDocumentTitle(t('layout:nav.updates'))

  const currentVersion = infoQuery.data?.build?.version
  const commit = infoQuery.data?.git?.commit?.id
  const releases = q.data ?? []
  // the current release floats to the top, the rest keeps server order (newest first)
  const sorted = [...releases].sort((a, b) => Number(b.version === currentVersion) - Number(a.version === currentVersion))

  return (
    <div className="max-w-3xl">
      <PageHeader title={t('layout:nav.updates')} subtitle={t('updates.subtitle')} />

      <section className="mb-6 rounded-xl border border-line bg-surface p-5">
        <h2 className="text-xs font-medium tracking-wide text-ink-3 uppercase">{t('updates.runningVersion')}</h2>
        {infoQuery.isPending ? (
          <Skeleton className="mt-3 h-7 w-40" />
        ) : (
          <div className="mt-2 flex flex-wrap items-center gap-x-4 gap-y-1">
            <span className="text-xl font-semibold text-ink">{currentVersion ?? t('common:state.unknown')}</span>
            {commit && (
              <span className="inline-flex items-center gap-1.5 font-mono text-xs text-ink-3">
                <GitCommit className="size-3.5" />
                {commit.slice(0, 8)}
              </span>
            )}
            {q.data && currentVersion && !releases.some((r) => r.version === currentVersion) && (
              <Chip className="px-2 py-0.5 text-[11px]">{t('updates.notInFeed')}</Chip>
            )}
          </div>
        )}
      </section>

      {q.isPending ? (
        <div className="flex flex-col gap-4">
          {Array.from({ length: 3 }, (_, i) => (
            <Skeleton key={i} className="h-32 rounded-xl" />
          ))}
        </div>
      ) : q.isLoadingError ? (
        <EmptyState
          icon={<WarningCircle />}
          title={t('updates.loadError')}
          body={q.error instanceof Error ? q.error.message : t('errorFallback')}
          action={<Button onClick={() => q.refetch()}>{t('common:action.retry')}</Button>}
        />
      ) : sorted.length === 0 ? (
        <EmptyState icon={<Rocket />} title={t('updates.emptyTitle')} body={t('updates.emptyBody')} />
      ) : (
        <div className="flex flex-col gap-4">
          {sorted.map((r) => (
            <ReleaseCard key={r.version} release={r} isCurrent={r.version === currentVersion} />
          ))}
        </div>
      )}
    </div>
  )
}
