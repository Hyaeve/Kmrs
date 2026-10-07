import { useEffect, useRef, useState } from 'react'
import { Reorder, useDragControls } from 'motion/react'
import { useTranslation } from 'react-i18next'
import { DotsSixVertical } from '@phosphor-icons/react'
import { cn } from '@/lib/utils/cn'
import { useChanged } from '@/lib/hooks/useChanged'
import { Dialog } from '@/components/ui/Dialog'
import { Switch } from '@/components/ui/Switch'
import { Button } from '@/components/ui/Button'
import { Skeleton } from '@/components/ui/Skeleton'
import { DASHBOARD_SECTIONS } from './sections'
import { useDashboardSections, type ResolvedDashboardSection } from './sectionConfig'

interface DashboardSectionsDialogProps {
  libraryId?: string
  open: boolean
  onOpenChange: (open: boolean) => void
}

function SectionRow({
  section,
  onToggle,
  onDragEnd,
  disabled,
}: {
  section: ResolvedDashboardSection
  onToggle: (hidden: boolean) => void
  onDragEnd: () => void
  disabled: boolean
}) {
  const controls = useDragControls()
  const { t } = useTranslation('dashboard')
  const title = t(DASHBOARD_SECTIONS[section.key].titleKey)

  return (
    <Reorder.Item
      value={section}
      dragListener={false}
      dragControls={controls}
      onDragEnd={onDragEnd}
      className={cn(
        'flex items-center gap-2 rounded-lg border border-line bg-surface px-2 py-2 select-none',
        section.hidden && 'opacity-60',
      )}
    >
      <button
        type="button"
        aria-label={t('sectionsDialog.reorder', { title })}
        onPointerDown={(e) => controls.start(e)}
        className="cursor-grab touch-none rounded-md p-1.5 text-ink-3 transition-colors hover:bg-raised hover:text-ink-2 active:cursor-grabbing"
      >
        <DotsSixVertical className="size-4" />
      </button>
      <p className="min-w-0 flex-1 truncate text-sm text-ink">{title}</p>
      <Switch checked={!section.hidden} onCheckedChange={(visible) => onToggle(!visible)} label={title} disabled={disabled} />
    </Reorder.Item>
  )
}

export function DashboardSectionsDialog({ libraryId, open, onOpenChange }: DashboardSectionsDialogProps) {
  const { t } = useTranslation('dashboard')
  const { sections, save, reset, isLoading, isSaving } = useDashboardSections(libraryId)
  const [draft, setDraft] = useState<ResolvedDashboardSection[]>(sections)
  const draftRef = useRef(draft)

  if (useChanged([open, sections]) && open) setDraft(sections)

  useEffect(() => {
    draftRef.current = draft
  }, [draft])

  const handleReorder = (next: ResolvedDashboardSection[]) => {
    draftRef.current = next
    setDraft(next)
  }

  const handleToggle = (key: string, hidden: boolean) => {
    const next = draft.map((s) => (s.key === key ? { ...s, hidden } : s))
    draftRef.current = next
    setDraft(next)
    save(next)
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange} title={t('customizeSections')} size="sm">
      <p className="px-5 pt-4 text-xs leading-relaxed text-ink-3">
        {libraryId ? t('sectionsDialog.bodyLibrary') : t('sectionsDialog.bodyAll')}
      </p>
      <div className="px-5 py-4">
        {isLoading ? (
          <div className="space-y-2">
            <Skeleton className="h-10 w-full" />
            <Skeleton className="h-10 w-full" />
            <Skeleton className="h-10 w-full" />
          </div>
        ) : (
          <Reorder.Group axis="y" values={draft} onReorder={handleReorder} className="flex flex-col gap-2">
            {draft.map((section) => (
              <SectionRow
                key={section.key}
                section={section}
                onToggle={(hidden) => handleToggle(section.key, hidden)}
                onDragEnd={() => save(draftRef.current)}
                disabled={isSaving}
              />
            ))}
          </Reorder.Group>
        )}
      </div>
      <div className="flex items-center justify-end gap-2 border-t border-line px-5 py-3.5">
        <Button variant="ghost" size="sm" onClick={() => reset()} disabled={isSaving}>
          {t('sectionsDialog.reset')}
        </Button>
      </div>
    </Dialog>
  )
}
