import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { AnimatePresence, motion, useReducedMotion } from 'motion/react'
import { CaretLeft, X } from '@phosphor-icons/react'
import { Button } from '@/components/ui/Button'
import { IconButton } from '@/components/ui/IconButton'
import type { ReferentialScope } from '@/lib/api/referential'
import { EnumFilterSection, FilterGroupDetail, FilterGroupRow, FlagFilterRow } from './FilterGroups'
import { groupSelectedCount } from './filterUrl'
import type { AuthorFilter, FilterGroupDef, FilterState, GroupKey, GroupMode } from './types'

interface FilterDrawerProps {
  open: boolean
  onClose: () => void
  groups: FilterGroupDef[]
  state: FilterState
  scope?: ReferentialScope
  /** resolves entity ids (libraries/read lists/collections) to display names in group summaries */
  labelFor?: (key: GroupKey, id: string) => string
  activeCount: number
  onToggleValue: (key: GroupKey, value: string) => void
  onToggleAuthor: (author: AuthorFilter) => void
  onSetMode: (key: GroupKey, mode: GroupMode) => void
  onSetNegated: (key: GroupKey, negated: boolean) => void
  onSetExclusive: (key: GroupKey, value: string) => void
  onCycleValue: (key: GroupKey, options: string[]) => void
  onClearGroup: (key: GroupKey) => void
  onClearAll: () => void
}

export function FilterDrawer({
  open,
  onClose,
  groups,
  state,
  scope,
  labelFor,
  activeCount,
  onToggleValue,
  onToggleAuthor,
  onSetMode,
  onSetNegated,
  onSetExclusive,
  onCycleValue,
  onClearGroup,
  onClearAll,
}: FilterDrawerProps) {
  const { t } = useTranslation('filters')
  const reduce = useReducedMotion()
  const [openKey, setOpenKey] = useState<GroupKey | null>(null)
  const openRef = useRef(open)

  useEffect(() => {
    openRef.current = open
  }, [open])

  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [open, onClose])

  useEffect(() => {
    if (!open) return
    const prev = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    return () => {
      document.body.style.overflow = prev
    }
  }, [open])

  const enumGroups = groups.filter((g) => g.kind === 'enum' && !g.flag)
  const flagGroups = groups.filter((g) => g.flag)
  // first-letter navigation is not metadata; it gets a standalone row
  const letterGroups = groups.filter((g) => g.kind === 'letters')
  const navGroups = groups.filter((g) => g.kind !== 'enum' && g.kind !== 'letters' && !g.flag)
  const openDef = groups.find((g) => g.key === openKey)

  return (
    // a quick close→reopen keeps the fresh drill-down; only reset when the drawer stayed closed through the exit
    <AnimatePresence
      onExitComplete={() => {
        if (!openRef.current) setOpenKey(null)
      }}
    >
      {open && (
        <>
          <motion.div
            key="overlay"
            className="fixed inset-0 z-20 bg-black/60"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: reduce ? 0 : 0.15 }}
            onClick={onClose}
          />
          <motion.aside
            key="panel"
            aria-label={t('title')}
            className="fixed inset-y-0 right-0 z-20 flex w-80 safe-top safe-bottom flex-col border-l border-line bg-surface sm:w-96"
            initial={reduce ? { opacity: 0 } : { x: '100%' }}
            animate={reduce ? { opacity: 1 } : { x: 0 }}
            exit={reduce ? { opacity: 0 } : { x: '100%' }}
            transition={reduce ? { duration: 0 } : { type: 'spring', stiffness: 380, damping: 36 }}
          >
            <div className="flex h-14 shrink-0 items-center gap-1 border-b border-line px-4">
              {openDef ? (
                <>
                  <IconButton label={t('common:action.back')} onClick={() => setOpenKey(null)} className="-ml-2">
                    <CaretLeft className="size-5" />
                  </IconButton>
                  <h2 className="min-w-0 flex-1 truncate font-display text-lg font-semibold text-ink">{t(openDef.labelKey)}</h2>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => onClearGroup(openDef.key)}
                    disabled={groupSelectedCount(openDef, state) === 0}
                  >
                    {t('resetGroup')}
                  </Button>
                </>
              ) : (
                <>
                  <h2 className="flex-1 font-display text-lg font-semibold text-ink">{t('title')}</h2>
                  <Button variant="ghost" size="sm" onClick={onClearAll} disabled={activeCount === 0}>
                    {t('clearAll')}
                  </Button>
                  <IconButton label={t('close')} onClick={onClose}>
                    <X className="size-5" />
                  </IconButton>
                </>
              )}
            </div>
            <div className="min-h-0 flex-1">
              <AnimatePresence mode="wait" initial={false}>
                <motion.div
                  key={openDef?.key ?? 'main'}
                  className="h-full overflow-y-auto px-4"
                  initial={reduce ? { opacity: 0 } : { opacity: 0, x: openKey ? 32 : -32 }}
                  animate={reduce ? { opacity: 1 } : { opacity: 1, x: 0 }}
                  exit={reduce ? { opacity: 0 } : { opacity: 0, x: openKey ? 32 : -32 }}
                  transition={{ duration: reduce ? 0.1 : 0.15 }}
                >
                  {openDef ? (
                    <FilterGroupDetail
                      def={openDef}
                      state={state}
                      scope={scope}
                      enabled={open}
                      onToggleValue={onToggleValue}
                      onToggleAuthor={onToggleAuthor}
                      onSetMode={onSetMode}
                      onSetNegated={onSetNegated}
                      onSetExclusive={onSetExclusive}
                    />
                  ) : (
                    <>
                      {enumGroups.map((def) => (
                        <EnumFilterSection
                          key={def.key}
                          def={def}
                          state={state}
                          onToggleValue={onToggleValue}
                          onSetMode={onSetMode}
                          onSetNegated={onSetNegated}
                        />
                      ))}
                      {flagGroups.length > 0 && (
                        <section className="border-b border-line py-4">
                          <h3 className="mb-1 text-[11px] font-medium tracking-[0.08em] text-ink-3 uppercase">
                            {t('section.flags')}
                          </h3>
                          {flagGroups.map((def) => (
                            <FlagFilterRow
                              key={def.key}
                              def={def}
                              value={(state[def.key] as string[])[0]}
                              onCycle={() => onCycleValue(def.key, (def.options ?? []).map((o) => o.value))}
                            />
                          ))}
                        </section>
                      )}
                      {letterGroups.map((def) => (
                        <section key={def.key} className="border-b border-line py-4">
                          <FilterGroupRow def={def} state={state} labelFor={labelFor} onOpen={() => setOpenKey(def.key)} />
                        </section>
                      ))}
                      {navGroups.length > 0 && (
                        <section className="py-4">
                          <h3 className="mb-1 text-[11px] font-medium tracking-[0.08em] text-ink-3 uppercase">
                            {t('section.metadata')}
                          </h3>
                          {navGroups.map((def) => (
                            <FilterGroupRow key={def.key} def={def} state={state} labelFor={labelFor} onOpen={() => setOpenKey(def.key)} />
                          ))}
                        </section>
                      )}
                    </>
                  )}
                </motion.div>
              </AnimatePresence>
            </div>
          </motion.aside>
        </>
      )}
    </AnimatePresence>
  )
}
