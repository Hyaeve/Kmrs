import { useMemo, useState, type ReactNode } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Reorder, useDragControls } from 'motion/react'
import { useTranslation } from 'react-i18next'
import { DotsSixVertical, X } from '@phosphor-icons/react'
import { collectionsApi } from '@/lib/api/collections'
import type { CollectionDto, SeriesDto } from '@/lib/api/types'
import { urls } from '@/lib/utils/urls'
import { cn } from '@/lib/utils/cn'
import { Button } from '@/components/ui/Button'
import { IconButton } from '@/components/ui/IconButton'
import { Switch } from '@/components/ui/Switch'
import { Skeleton } from '@/components/ui/Skeleton'
import { EmptyState } from '@/components/ui/EmptyState'
import { CoverImage } from '@/components/media/CoverImage'

interface EditCollectionMembersProps {
  collection: CollectionDto
  onExit: () => void
}

export function EditCollectionMembers({ collection, onExit }: EditCollectionMembersProps) {
  const { t } = useTranslation('detail')
  const queryClient = useQueryClient()
  const [members, setMembers] = useState<SeriesDto[]>([])
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [draggingId, setDraggingId] = useState<string | null>(null)
  // init from the query once; later refetches (e.g. after toggling ordering) must not wipe unsaved edits
  const [initialized, setInitialized] = useState(false)

  // reorder needs the full member list, not the paged grid data
  const membersQuery = useQuery({
    queryKey: ['collections', collection.id, 'series', 'edit-all'],
    queryFn: () => collectionsApi.series(collection.id, { unpaged: true }),
  })

  if (membersQuery.data && !initialized) {
    setMembers(membersQuery.data.content)
    setInitialized(true)
  }

  const byId = useMemo(() => new Map(members.map((m) => [m.id, m])), [members])
  const memberIds = useMemo(() => members.map((m) => m.id), [members])

  const saveMutation = useMutation({
    mutationFn: () => collectionsApi.update(collection.id, { seriesIds: memberIds }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['collections'] })
      onExit()
    },
  })

  const orderedMutation = useMutation({
    mutationFn: (ordered: boolean) => collectionsApi.update(collection.id, { ordered }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['collections'] }),
  })

  const busy = saveMutation.isPending || orderedMutation.isPending
  // membership always counts; order only matters when manual ordering is on
  const dirty = useMemo(() => {
    const saved = collection.seriesIds
    if (memberIds.length !== saved.length) return true
    const savedSet = new Set(saved)
    if (memberIds.some((id) => !savedSet.has(id))) return true
    return collection.ordered && memberIds.some((id, i) => saved[i] !== id)
  }, [memberIds, collection])

  const toggleAll = () =>
    setSelected((prev) => (prev.size === members.length ? new Set() : new Set(members.map((m) => m.id))))
  const removeIds = (ids: Set<string>) => {
    setMembers((prev) => prev.filter((m) => !ids.has(m.id)))
    setSelected((prev) => new Set([...prev].filter((id) => !ids.has(id))))
  }
  const toggleSelected = (id: string, v: boolean) =>
    setSelected((prev) => {
      const next = new Set(prev)
      if (v) next.add(id)
      else next.delete(id)
      return next
    })

  return (
    <div>
      <div className="mb-4 flex flex-wrap items-center gap-x-4 gap-y-2 rounded-xl border border-line bg-surface px-4 py-3">
        <p className="text-sm text-ink-2">
          {selected.size > 0
            ? t('collection.selectedSeriesCount', { count: selected.size })
            : t('seriesCount', { count: members.length })}
        </p>
        <button
          type="button"
          onClick={toggleAll}
          disabled={busy || members.length === 0}
          className="cursor-pointer text-sm text-accent-strong transition-colors hover:text-accent disabled:pointer-events-none disabled:opacity-50"
        >
          {selected.size === members.length && members.length > 0 ? t('editBooks.clearSelection') : t('editBooks.selectAll')}
        </button>
        <Button variant="danger" size="sm" disabled={busy || selected.size === 0} onClick={() => removeIds(selected)}>
          {t('editBooks.removeSelected')}
        </Button>
        <div className="ml-auto flex items-center gap-2">
          <span className="text-sm text-ink-2">{t('editBooks.manualOrdering')}</span>
          <Switch
            checked={collection.ordered}
            onCheckedChange={(v) => orderedMutation.mutate(v)}
            disabled={busy}
            label={t('editBooks.manualOrdering')}
          />
        </div>
        <Button variant="ghost" size="sm" disabled={busy} onClick={onExit}>
          {t('common:action.cancel')}
        </Button>
        <Button
          variant="primary"
          size="sm"
          loading={saveMutation.isPending}
          disabled={busy || members.length === 0 || !dirty}
          onClick={() => saveMutation.mutate()}
        >
          {t('common:action.save')}
        </Button>
      </div>
      {members.length === 0 && !membersQuery.isPending && (
        <p className="mb-4 text-sm text-danger">
          {t('collection.editMembersEmpty')}
        </p>
      )}
      {saveMutation.error && <p className="mb-4 text-sm text-danger">{saveMutation.error.message}</p>}

      {membersQuery.isPending ? (
        <div className="space-y-2">
          {Array.from({ length: 6 }, (_, i) => (
            <Skeleton key={i} className="h-14" />
          ))}
        </div>
      ) : membersQuery.isLoadingError ? (
        <EmptyState
          title={t('empty.loadSeriesFailed')}
          body={membersQuery.error.message}
          action={
            <Button variant="secondary" onClick={() => membersQuery.refetch()}>
              {t('common:action.retry')}
            </Button>
          }
        />
      ) : collection.ordered ? (
        <Reorder.Group
          axis="y"
          values={memberIds}
          onReorder={(ids) => setMembers(ids.map((id) => byId.get(id)).filter((m): m is SeriesDto => !!m))}
          className="space-y-2"
        >
          {memberIds.map((id, i) => (
            <SortableRow
              key={id}
              id={id}
              index={i}
              disabled={busy}
              dragging={draggingId === id}
              onDragStart={() => setDraggingId(id)}
              onDragEnd={() => setDraggingId(null)}
              checked={selected.has(id)}
              onCheckedChange={(v) => toggleSelected(id, v)}
              onRemove={() => removeIds(new Set([id]))}
            >
              <SeriesCell series={byId.get(id)} />
            </SortableRow>
          ))}
        </Reorder.Group>
      ) : (
        <div className="space-y-2">
          {members.map((m) => (
            <RowFrame
              key={m.id}
              disabled={busy}
              checked={selected.has(m.id)}
              onCheckedChange={(v) => toggleSelected(m.id, v)}
              onRemove={() => removeIds(new Set([m.id]))}
            >
              <SeriesCell series={m} />
            </RowFrame>
          ))}
        </div>
      )}
    </div>
  )
}

function SeriesCell({ series }: { series?: SeriesDto }) {
  const { t } = useTranslation('detail')
  if (!series) return null
  const title = series.metadata.title || series.name
  return (
    <>
      <CoverImage src={urls.seriesThumbnail(series.id)} alt={title} className="w-9 shrink-0" />
      <div className="min-w-0 flex-1">
        <p className="truncate text-sm font-medium text-ink">{title}</p>
        <p className="text-xs text-ink-3">{t('bookCount', { count: series.booksCount })}</p>
      </div>
    </>
  )
}

interface RowFrameProps {
  index?: number
  disabled: boolean
  checked: boolean
  onCheckedChange: (v: boolean) => void
  onRemove: () => void
  children: ReactNode
  className?: string
  handle?: ReactNode
}

function RowFrame({ index, disabled, checked, onCheckedChange, onRemove, children, className, handle }: RowFrameProps) {
  const { t } = useTranslation('detail')
  return (
    <div className={cn('flex items-center gap-3 rounded-xl border border-line bg-surface px-3 py-2', className)}>
      {handle}
      <input
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(e) => onCheckedChange(e.target.checked)}
        aria-label={t('editBooks.selectRow')}
        className="size-4 shrink-0 cursor-pointer accent-accent disabled:opacity-40"
      />
      {index !== undefined && <span className="w-7 shrink-0 text-center font-mono text-xs text-ink-3">{index + 1}</span>}
      {children}
      <IconButton label={t('common:action.remove')} disabled={disabled} onClick={onRemove} className="size-8">
        <X className="size-4" />
      </IconButton>
    </div>
  )
}

interface SortableRowProps extends Omit<RowFrameProps, 'className' | 'handle' | 'index'> {
  id: string
  index: number
  dragging: boolean
  onDragStart: () => void
  onDragEnd: () => void
}

function SortableRow({ id, dragging, onDragStart, onDragEnd, ...rest }: SortableRowProps) {
  const { t } = useTranslation('detail')
  const controls = useDragControls()
  return (
    <Reorder.Item
      value={id}
      dragListener={false}
      dragControls={controls}
      onDragStart={onDragStart}
      onDragEnd={onDragEnd}
      className={cn('relative', dragging && 'z-10')}
    >
      <RowFrame
        {...rest}
        className={cn(dragging && 'border-line-strong bg-overlay shadow-pop')}
        handle={
          <button
            type="button"
            aria-label={t('editBooks.dragToReorder')}
            onPointerDown={(e) => controls.start(e)}
            className={cn(
              '-ml-1 shrink-0 cursor-grab touch-none rounded-md p-1 text-ink-3 transition-colors hover:bg-raised hover:text-ink',
              dragging && 'cursor-grabbing text-accent',
            )}
          >
            <DotsSixVertical className="size-4" />
          </button>
        }
      />
    </Reorder.Item>
  )
}
