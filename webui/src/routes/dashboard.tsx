import { useState, type ReactNode } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { useInfiniteQuery, useQuery, type InfiniteData } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Books, PushPin, SlidersHorizontal } from '@phosphor-icons/react'
import { librariesApi } from '@/lib/api/libraries'
import type { BookDto, LibraryDto, Page, SeriesDto } from '@/lib/api/types'
import { cn } from '@/lib/utils/cn'
import { useDocumentTitle } from '@/lib/hooks/useDocumentTitle'
import { usePinnedLibraries } from '@/lib/store/clientSettings'
import { EmptyState } from '@/components/ui/EmptyState'
import { IconButton } from '@/components/ui/IconButton'
import { Button } from '@/components/ui/Button'
import { DashboardRow, type PagedRowQuery } from '@/components/dashboard/DashboardRow'
import { DASHBOARD_SECTIONS, type DashboardSectionKey } from '@/components/dashboard/sections'
import { dashboardLibraryIds, dashboardScope, useDashboardSections } from '@/components/dashboard/sectionConfig'
import { DashboardSectionsDialog } from '@/components/dashboard/DashboardSectionsDialog'
import { PinnedLibrariesEmpty } from '@/components/dashboard/PinnedLibrariesEmpty'
import { PinLibrariesDialog } from '@/components/layout/PinLibrariesDialog'

function usePagedRow<T>(
  queryKey: readonly unknown[],
  fetchPage: (page: number) => Promise<Page<T>>,
  enabled: boolean,
): PagedRowQuery<T> {
  const query = useInfiniteQuery<Page<T>, Error, InfiniteData<Page<T>, number>, readonly unknown[], number>({
    queryKey,
    queryFn: ({ pageParam }) => fetchPage(pageParam),
    getNextPageParam: (last) => (last.last ? undefined : last.number + 1),
    initialPageParam: 0,
    enabled,
  })
  return { ...query, items: query.data?.pages.flatMap((p) => p.content) ?? [] }
}

function useSectionRow(key: DashboardSectionKey, libraryIds: string[] | undefined, scope: string, enabled: boolean) {
  return usePagedRow(['dashboard', key, scope], (page) => DASHBOARD_SECTIONS[key].fetchPage(libraryIds, page), enabled)
}

function ScopeButton({ active, onClick, children }: { active: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        'flex cursor-pointer items-center gap-1.5 rounded-md px-3 py-1.5 text-[13px] font-medium whitespace-nowrap transition-all duration-150',
        active ? 'bg-raised text-ink shadow-sm' : 'text-ink-3 hover:text-ink-2',
      )}
    >
      {children}
    </button>
  )
}

/** SegmentedControl look, plus pin markers: pinned libraries lead in their pinned order */
function LibraryScopeSwitcher({
  libraries,
  pinnedIds,
  value,
  onChange,
}: {
  libraries: LibraryDto[]
  pinnedIds: string[]
  value: string
  onChange: (v: string) => void
}) {
  const { t } = useTranslation('dashboard')
  const pinnedSet = new Set(pinnedIds)
  const ordered = [
    ...pinnedIds.map((id) => libraries.find((l) => l.id === id)).filter((l) => !!l),
    ...libraries.filter((l) => !pinnedSet.has(l.id)),
  ]
  return (
    <div className="inline-flex items-center gap-0.5 rounded-lg border border-line bg-surface p-0.5">
      <ScopeButton active={value === 'all'} onClick={() => onChange('all')}>
        {t('scope.all')}
      </ScopeButton>
      {ordered.map((lib) => (
        <ScopeButton key={lib.id} active={value === lib.id} onClick={() => onChange(lib.id)}>
          {pinnedSet.has(lib.id) && <PushPin weight="fill" className="size-3 shrink-0" />}
          <span className="truncate">{lib.name}</span>
        </ScopeButton>
      ))}
    </div>
  )
}

export function DashboardPage() {
  const { t } = useTranslation('dashboard')
  const { libraryId } = useParams()
  const navigate = useNavigate()

  const librariesQuery = useQuery({ queryKey: ['libraries'], queryFn: librariesApi.list })
  const libraries = librariesQuery.data
  const library = libraryId ? libraries?.find((l) => l.id === libraryId) : undefined

  // pinned stays undefined until settings load; don't fire unfiltered section queries in that window
  const { pinned, isPending: settingsPending } = usePinnedLibraries()
  const libraryIds = dashboardLibraryIds(libraryId, pinned)
  const scope = dashboardScope(libraryId, pinned)
  const pinnedEmpty = !libraryId && pinned !== undefined && pinned.length === 0
  const rowsEnabled = !settingsPending && !pinnedEmpty

  const { sections } = useDashboardSections(libraryId)
  const [sectionsDialogOpen, setSectionsDialogOpen] = useState(false)
  const [pinDialogOpen, setPinDialogOpen] = useState(false)

  useDocumentTitle(libraryId ? library?.name : t('title'))

  const sectionQueries: Record<DashboardSectionKey, PagedRowQuery<BookDto | SeriesDto>> = {
    'keep-reading': useSectionRow('keep-reading', libraryIds, scope, rowsEnabled),
    'on-deck': useSectionRow('on-deck', libraryIds, scope, rowsEnabled),
    'recently-released-books': useSectionRow('recently-released-books', libraryIds, scope, rowsEnabled),
    'recently-added-books': useSectionRow('recently-added-books', libraryIds, scope, rowsEnabled),
    'recently-added-series': useSectionRow('recently-added-series', libraryIds, scope, rowsEnabled),
    'recently-updated-series': useSectionRow('recently-updated-series', libraryIds, scope, rowsEnabled),
    'recently-read': useSectionRow('recently-read', libraryIds, scope, rowsEnabled),
  }

  const visibleSections = sections.filter((s) => !s.hidden)
  const rows = visibleSections.map((s) => ({ key: s.key, query: sectionQueries[s.key] }))
  const allHidden = visibleSections.length === 0
  const allEmpty = rows.length > 0 && rows.every((r) => r.query.isSuccess && r.query.items.length === 0)

  return (
    <div>
      <div className="mb-6 flex items-center gap-2">
        {libraries && libraries.length > 1 && (
          <div className="min-w-0 flex-1 overflow-x-auto">
            <LibraryScopeSwitcher
              libraries={libraries}
              pinnedIds={pinned ?? []}
              value={libraryId ?? 'all'}
              onChange={(v) => navigate(v === 'all' ? '/dashboard' : `/libraries/${v}/recommended`)}
            />
          </div>
        )}
        <IconButton label={t('customizeSections')} onClick={() => setSectionsDialogOpen(true)}>
          <SlidersHorizontal className="size-5" />
        </IconButton>
      </div>

      {pinnedEmpty ? (
        <PinnedLibrariesEmpty onManage={() => setPinDialogOpen(true)} />
      ) : allHidden ? (
        <EmptyState
          icon={<SlidersHorizontal />}
          title={t('allHidden.title')}
          body={t('allHidden.body')}
          action={
            <Button variant="primary" onClick={() => setSectionsDialogOpen(true)}>
              {t('customizeSections')}
            </Button>
          }
        />
      ) : allEmpty ? (
        <EmptyState
          icon={<Books />}
          title={t('empty.title')}
          body={t('empty.body')}
        />
      ) : (
        <div className="space-y-8">
          {rows.map(({ key, query }) => (
            <DashboardRow
              key={key}
              title={t(DASHBOARD_SECTIONS[key].titleKey)}
              to={libraryId ? `/libraries/${libraryId}/sections/${key}` : `/dashboard/sections/${key}`}
              query={query}
              keyOf={(item) => item.id}
              renderItem={DASHBOARD_SECTIONS[key].renderRow}
              skeleton={DASHBOARD_SECTIONS[key].renderSkeleton?.()}
            />
          ))}
        </div>
      )}

      <DashboardSectionsDialog libraryId={libraryId} open={sectionsDialogOpen} onOpenChange={setSectionsDialogOpen} />
      <PinLibrariesDialog open={pinDialogOpen} onOpenChange={setPinDialogOpen} />
    </div>
  )
}
