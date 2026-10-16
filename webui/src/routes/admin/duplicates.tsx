import { useMemo, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Copy, WarningCircle } from '@phosphor-icons/react'
import { booksApi } from '@/lib/api/books'
import { librariesApi } from '@/lib/api/libraries'
import type { BookDto } from '@/lib/api/types'
import { useDocumentTitle } from '@/lib/hooks/useDocumentTitle'
import { Button } from '@/components/ui/Button'
import { EmptyState } from '@/components/ui/EmptyState'
import { PageHeader } from '@/components/ui/PageHeader'
import { Skeleton } from '@/components/ui/Skeleton'
import { DeleteBookFileDialog } from '@/components/admin/duplicates/DeleteBookFileDialog'
import { DuplicateGroup } from '@/components/admin/duplicates/DuplicateGroup'

export function AdminDuplicatesPage() {
  const { t } = useTranslation('admin-maintenance')
  const [deleting, setDeleting] = useState<BookDto | null>(null)

  useDocumentTitle(t('layout:nav.duplicates'))

  // unpaged: groups can span pages, so grouping must happen on the full set
  const q = useQuery({
    queryKey: ['admin', 'duplicates'],
    queryFn: () => booksApi.duplicates({ unpaged: true }),
  })
  const librariesQuery = useQuery({ queryKey: ['libraries'], queryFn: librariesApi.list })

  const groups = useMemo(() => {
    const byHash = new Map<string, BookDto[]>()
    for (const b of q.data?.content ?? []) {
      const list = byHash.get(b.fileHash) ?? []
      list.push(b)
      byHash.set(b.fileHash, list)
    }
    return [...byHash.entries()]
      .filter(([, books]) => books.length > 1)
      .sort((a, b) => b[1].length * b[1][0].sizeBytes - a[1].length * a[1][0].sizeBytes)
  }, [q.data])

  const bookCount = groups.reduce((n, [, books]) => n + books.length, 0)

  return (
    <div className="max-w-5xl">
      <PageHeader
        title={t('layout:nav.duplicates')}
        subtitle={
          q.data
            ? `${t('duplicates.groupCount', { count: groups.length })} · ${t('duplicates.sharingIdentical', { count: bookCount })}`
            : t('duplicates.subtitleFallback')
        }
      />

      {q.isPending ? (
        <div className="flex flex-col gap-4">
          {Array.from({ length: 3 }, (_, i) => (
            <Skeleton key={i} className="h-40 rounded-xl" />
          ))}
        </div>
      ) : q.isLoadingError ? (
        <EmptyState
          icon={<WarningCircle />}
          title={t('duplicates.loadError')}
          body={q.error instanceof Error ? q.error.message : t('errorFallback')}
          action={<Button onClick={() => q.refetch()}>{t('common:action.retry')}</Button>}
        />
      ) : groups.length === 0 ? (
        <EmptyState
          icon={<Copy />}
          title={t('duplicates.emptyTitle')}
          body={t('duplicates.emptyBody')}
        />
      ) : (
        <div className="flex flex-col gap-4">
          {groups.map(([hash, books]) => (
            <DuplicateGroup
              key={hash}
              fileHash={hash}
              books={books}
              libraries={librariesQuery.data ?? []}
              onDelete={setDeleting}
            />
          ))}
        </div>
      )}

      <DeleteBookFileDialog
        book={deleting}
        onOpenChange={(o) => {
          if (!o) setDeleting(null)
        }}
      />
    </div>
  )
}
