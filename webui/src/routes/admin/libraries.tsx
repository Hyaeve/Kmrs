import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Books, Plus, WarningCircle } from '@phosphor-icons/react'
import { librariesApi } from '@/lib/api/libraries'
import type { LibraryDto } from '@/lib/api/types'
import { useDocumentTitle } from '@/lib/hooks/useDocumentTitle'
import { Button } from '@/components/ui/Button'
import { EmptyState } from '@/components/ui/EmptyState'
import { PageHeader } from '@/components/ui/PageHeader'
import { Skeleton } from '@/components/ui/Skeleton'
import { DeleteLibraryDialog } from '@/components/admin/libraries/DeleteLibraryDialog'
import { LibraryCard } from '@/components/admin/libraries/LibraryCard'
import { LibraryDialog } from '@/components/admin/libraries/LibraryDialog'

const GRID_STYLE = { gridTemplateColumns: 'repeat(auto-fill, minmax(300px, 1fr))' }

export function AdminLibrariesPage() {
  const { t } = useTranslation('admin-maintenance')
  const [addOpen, setAddOpen] = useState(false)
  const [editing, setEditing] = useState<LibraryDto | null>(null)
  const [deleting, setDeleting] = useState<LibraryDto | null>(null)

  useDocumentTitle(t('layout:nav.libraries'))

  const q = useQuery({ queryKey: ['libraries'], queryFn: librariesApi.list })
  const libraries = q.data ?? []

  return (
    <div>
      <PageHeader
        title={t('layout:nav.libraries')}
        subtitle={q.data ? t('libraries.subtitle', { count: q.data.length }) : undefined}
        actions={
          <Button variant="primary" onClick={() => setAddOpen(true)}>
            <Plus className="size-4" />
            {t('libraries.add')}
          </Button>
        }
      />
      {q.isPending ? (
        <div className="grid gap-4" style={GRID_STYLE}>
          {Array.from({ length: 3 }, (_, i) => (
            <Skeleton key={i} className="h-36 rounded-xl" />
          ))}
        </div>
      ) : q.isLoadingError ? (
        <EmptyState
          icon={<WarningCircle />}
          title={t('libraries.loadError')}
          body={q.error instanceof Error ? q.error.message : t('errorFallback')}
          action={<Button onClick={() => q.refetch()}>{t('common:action.retry')}</Button>}
        />
      ) : libraries.length === 0 ? (
        <EmptyState
          icon={<Books />}
          title={t('layout:nav.noLibraries')}
          body={t('libraries.emptyBody')}
          action={
            <Button variant="primary" onClick={() => setAddOpen(true)}>
              <Plus className="size-4" />
              {t('libraries.add')}
            </Button>
          }
        />
      ) : (
        <div className="grid gap-4" style={GRID_STYLE}>
          {libraries.map((lib) => (
            <LibraryCard key={lib.id} library={lib} onEdit={setEditing} onDelete={setDeleting} />
          ))}
        </div>
      )}

      <LibraryDialog open={addOpen} onOpenChange={setAddOpen} />
      {editing && (
        <LibraryDialog
          key={editing.id}
          open
          onOpenChange={(o) => {
            if (!o) setEditing(null)
          }}
          library={editing}
        />
      )}
      <DeleteLibraryDialog
        library={deleting}
        onOpenChange={(o) => {
          if (!o) setDeleting(null)
        }}
      />
    </div>
  )
}
