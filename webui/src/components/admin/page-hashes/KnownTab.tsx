import { useState } from 'react'
import { keepPreviousData, useInfiniteQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Check, DotsThreeVertical, Funnel, Images, WarningCircle } from '@phosphor-icons/react'
import { pageHashesApi } from '@/lib/api/pageHashes'
import type { PageHashAction, PageHashKnownDto } from '@/lib/api/types'
import { relativeTime } from '@/lib/utils/format'
import { Button } from '@/components/ui/Button'
import { EmptyState } from '@/components/ui/EmptyState'
import { IconButton } from '@/components/ui/IconButton'
import { Menu, MenuItem, MenuLabel, MenuSeparator } from '@/components/ui/Menu'
import { Skeleton } from '@/components/ui/Skeleton'
import { Sentinel } from '@/components/filters/Sentinel'
import { ACTION_LABEL_KEYS } from './actionMeta'
import { HashActionBadge } from './HashActionBadge'
import { DeleteAllPagesDialog } from './DeleteAllPagesDialog'
import { MatchesDialog } from './MatchesDialog'

const ACTIONS: PageHashAction[] = ['IGNORE', 'DELETE_MANUAL', 'DELETE_AUTO']

function RowActions({
  known,
  onViewMatches,
  onDeleteAll,
}: {
  known: PageHashKnownDto
  onViewMatches: () => void
  onDeleteAll: () => void
}) {
  const { t } = useTranslation('admin-maintenance')
  const queryClient = useQueryClient()

  const setAction = useMutation({
    mutationFn: (action: PageHashAction) =>
      pageHashesApi.createOrUpdate({ hash: known.hash, size: known.size, action }),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: ['admin', 'page-hashes'] }),
  })

  return (
    <Menu
      trigger={
        <IconButton label={t('pageHashes.actionsForHash', { hash: known.hash })}>
          <DotsThreeVertical className="size-4" />
        </IconButton>
      }
    >
      <MenuLabel>{t('pageHashes.setAction')}</MenuLabel>
      {ACTIONS.map((a) => (
        <MenuItem key={a} onSelect={() => setAction.mutate(a)} disabled={setAction.isPending}>
          <span className="flex-1">{t(ACTION_LABEL_KEYS[a])}</span>
          {a === known.action && <Check className="size-4 text-accent" />}
        </MenuItem>
      ))}
      <MenuSeparator />
      <MenuItem onSelect={onViewMatches}>
        <Images className="size-4" /> {t('pageHashes.viewMatches')}
      </MenuItem>
      <MenuSeparator />
      <MenuItem danger onSelect={onDeleteAll}>
        {t('pageHashes.deleteAllMatching')}
      </MenuItem>
    </Menu>
  )
}

export function KnownTab() {
  const { t } = useTranslation('admin-maintenance')
  const [actionFilter, setActionFilter] = useState<PageHashAction | null>(null)
  const [matchesHash, setMatchesHash] = useState<string | null>(null)
  const [deleting, setDeleting] = useState<PageHashKnownDto | null>(null)

  const q = useInfiniteQuery({
    queryKey: ['admin', 'page-hashes', 'known', actionFilter ?? 'all'],
    queryFn: ({ pageParam }) =>
      pageHashesApi.listKnown({ page: pageParam, size: 50, action: actionFilter ? [actionFilter] : undefined }),
    getNextPageParam: (last) => (last.last ? undefined : last.number + 1),
    initialPageParam: 0,
    placeholderData: keepPreviousData,
  })

  const items = q.data?.pages.flatMap((p) => p.content) ?? []
  const total = q.data?.pages[0]?.totalElements

  return (
    <div>
      <div className="mb-4 flex items-center justify-between gap-3">
        <p className="text-sm text-ink-3">
          {total !== undefined ? t('pageHashes.knownCount', { count: total }) : t('pageHashes.knownFallback')}
        </p>
        <Menu
          trigger={
            <Button variant="secondary" size="sm">
              <Funnel className="size-4" />
              {actionFilter ? t(ACTION_LABEL_KEYS[actionFilter]) : t('pageHashes.allActions')}
            </Button>
          }
        >
          <MenuItem onSelect={() => setActionFilter(null)}>
            <span className="flex-1">{t('pageHashes.allActions')}</span>
            {actionFilter === null && <Check className="size-4 text-accent" />}
          </MenuItem>
          {ACTIONS.map((a) => (
            <MenuItem key={a} onSelect={() => setActionFilter(a)}>
              <span className="flex-1">{t(ACTION_LABEL_KEYS[a])}</span>
              {actionFilter === a && <Check className="size-4 text-accent" />}
            </MenuItem>
          ))}
        </Menu>
      </div>

      {q.isPending ? (
        <div className="flex flex-col gap-2.5">
          {Array.from({ length: 5 }, (_, i) => (
            <Skeleton key={i} className="h-20 w-full" />
          ))}
        </div>
      ) : q.isLoadingError ? (
        <EmptyState
          icon={<WarningCircle />}
          title={t('pageHashes.loadKnownError')}
          body={q.error instanceof Error ? q.error.message : t('errorFallback')}
          action={<Button onClick={() => q.refetch()}>{t('common:action.retry')}</Button>}
        />
      ) : items.length === 0 ? (
        <EmptyState
          icon={<Images />}
          title={t('pageHashes.noKnown')}
          body={t('pageHashes.noKnownBody')}
        />
      ) : (
        <>
          <ul className="flex flex-col divide-y divide-line rounded-xl border border-line bg-surface">
            {items.map((k) => (
              <li key={k.hash} className="flex items-center gap-3 px-4 py-3">
                <div
                  aria-hidden
                  className="h-16 w-12 shrink-0 rounded-md bg-raised bg-cover bg-center"
                  style={{ backgroundImage: `url(${pageHashesApi.knownThumbnailUrl(k.hash)})` }}
                />
                <div className="min-w-0 flex-1">
                  <code className="block max-w-64 truncate font-mono text-xs text-ink-2" title={k.hash}>
                    {k.hash}
                  </code>
                  <p className="mt-1 text-xs text-ink-3">
                    {k.deleteCount > 0
                      ? t('pageHashes.rowMetaDeleted', {
                          count: k.matchCount,
                          deleted: t('pageHashes.pagesDeleted', { count: k.deleteCount }),
                          added: relativeTime(k.created),
                        })
                      : t('pageHashes.rowMeta', { count: k.matchCount, added: relativeTime(k.created) })}
                  </p>
                </div>
                <HashActionBadge action={k.action} />
                <Button size="sm" onClick={() => setMatchesHash(k.hash)}>
                  {t('pageHashes.matches')}
                </Button>
                <RowActions
                  known={k}
                  onViewMatches={() => setMatchesHash(k.hash)}
                  onDeleteAll={() => setDeleting(k)}
                />
              </li>
            ))}
          </ul>
          <Sentinel
            active={!!q.hasNextPage && !q.isPlaceholderData}
            onIntersect={() => {
              if (q.hasNextPage && !q.isFetchingNextPage) void q.fetchNextPage()
            }}
          />
          {q.isFetchingNextPage && <Skeleton className="mt-3 h-16 w-full" />}
        </>
      )}

      <MatchesDialog
        hash={matchesHash}
        onOpenChange={(o) => {
          if (!o) setMatchesHash(null)
        }}
      />
      <DeleteAllPagesDialog
        known={deleting}
        onOpenChange={(o) => {
          if (!o) setDeleting(null)
        }}
      />
    </div>
  )
}
