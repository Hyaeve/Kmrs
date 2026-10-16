import { keepPreviousData, useInfiniteQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { Link } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { Trash } from '@phosphor-icons/react'
import { pageHashesApi } from '@/lib/api/pageHashes'
import type { PageHashMatchDto } from '@/lib/api/types'
import { urls } from '@/lib/utils/urls'
import { formatBytes } from '@/lib/utils/format'
import { Button } from '@/components/ui/Button'
import { Dialog } from '@/components/ui/Dialog'
import { IconButton } from '@/components/ui/IconButton'
import { Skeleton } from '@/components/ui/Skeleton'
import { Sentinel } from '@/components/filters/Sentinel'

interface MatchesDialogProps {
  hash: string | null
  onOpenChange: (open: boolean) => void
}

function MatchCard({ hash, match }: { hash: string; match: PageHashMatchDto }) {
  const { t } = useTranslation('admin-maintenance')
  const queryClient = useQueryClient()

  const remove = useMutation({
    mutationFn: () => pageHashesApi.deleteMatch(hash, match),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['admin', 'page-hashes'] })
    },
  })

  return (
    <figure className="group relative overflow-hidden rounded-lg border border-line bg-raised">
      <Link to={`/book/${match.bookId}`} className="block">
        <div
          className="h-36 w-full bg-cover bg-center"
          style={{ backgroundImage: `url(${urls.bookPage(match.bookId, match.pageNumber)})` }}
        />
      </Link>
      <figcaption className="flex items-center gap-2 px-2.5 py-2">
        <div className="min-w-0 flex-1">
          <p className="truncate text-xs text-ink-2" title={match.fileName}>
            <Link to={`/book/${match.bookId}`} className="hover:text-accent-strong">
              {match.fileName}
            </Link>
          </p>
          <p className="text-[11px] text-ink-3">
            p.{match.pageNumber} · {formatBytes(match.fileSize)}
          </p>
        </div>
        <IconButton
          label={t('pageHashes.deletePageLabel')}
          className="size-8 text-danger hover:bg-danger/10"
          onClick={() => remove.mutate()}
          disabled={remove.isPending}
        >
          <Trash className="size-4" />
        </IconButton>
      </figcaption>
      {remove.isError && (
        <p className="px-2.5 pb-2 text-[11px] text-danger">
          {remove.error instanceof Error ? remove.error.message : t('pageHashes.deleteFailed')}
        </p>
      )}
    </figure>
  )
}

export function MatchesDialog({ hash, onOpenChange }: MatchesDialogProps) {
  const { t } = useTranslation('admin-maintenance')
  const q = useInfiniteQuery({
    queryKey: ['admin', 'page-hashes', 'matches', hash],
    queryFn: ({ pageParam }) => pageHashesApi.matches(hash!, { page: pageParam, size: 24 }),
    getNextPageParam: (last) => (last.last ? undefined : last.number + 1),
    initialPageParam: 0,
    enabled: !!hash,
    placeholderData: keepPreviousData,
  })

  const matches = q.data?.pages.flatMap((p) => p.content) ?? []

  return (
    <Dialog open={!!hash} onOpenChange={onOpenChange} title={t('pageHashes.matchesTitle')} size="lg">
      <div className="p-5">
        <p className="mb-4 truncate font-mono text-xs text-ink-3" title={hash ?? undefined}>
          {hash}
        </p>
        {q.isPending ? (
          <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4">
            {Array.from({ length: 8 }, (_, i) => (
              <Skeleton key={i} className="h-44" />
            ))}
          </div>
        ) : q.isLoadingError ? (
          <div className="flex items-center gap-3">
            <p className="text-sm text-danger">
              {q.error instanceof Error ? q.error.message : t('pageHashes.loadMatchesError')}
            </p>
            <Button size="sm" onClick={() => void q.refetch()}>
              {t('pageHashes.tryAgain')}
            </Button>
          </div>
        ) : matches.length === 0 ? (
          <p className="text-sm text-ink-3">{t('pageHashes.noMatches')}</p>
        ) : (
          <>
            <p className="mb-3 text-xs text-ink-3">{t('pageHashes.matchCount', { count: q.data.pages[0].totalElements })}</p>
            <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4">
              {matches.map((m) => (
                <MatchCard key={`${m.bookId}:${m.pageNumber}`} hash={hash!} match={m} />
              ))}
            </div>
            <Sentinel
              active={!!q.hasNextPage && !q.isPlaceholderData}
              onIntersect={() => {
                if (q.hasNextPage && !q.isFetchingNextPage) void q.fetchNextPage()
              }}
            />
            {q.isFetchingNextPage && <Skeleton className="mt-3 h-8 w-full" />}
          </>
        )}
      </div>
    </Dialog>
  )
}
