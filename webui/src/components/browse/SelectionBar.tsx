import type { ReactNode } from 'react'
import { AnimatePresence, motion, useReducedMotion } from 'motion/react'
import { Trans, useTranslation } from 'react-i18next'
import { CircleNotch, WarningCircle } from '@phosphor-icons/react'
import { Button } from '@/components/ui/Button'
import type { BatchRunState } from '@/components/selection/useBatchRun'

interface SelectionBarProps {
  count: number
  /** items currently loaded; caps "Select all" */
  loaded: number
  state: BatchRunState
  onSelectAll: () => void
  onClear: () => void
  onDismiss: () => void
  children: ReactNode
}

/** Floating batch-action bar pinned to the bottom of the viewport while a selection exists. */
export function SelectionBar({ count, loaded, state, onSelectAll, onClear, onDismiss, children }: SelectionBarProps) {
  const { t } = useTranslation('browse')
  const reduce = useReducedMotion()
  const busy = state.status === 'running'

  return (
    <AnimatePresence>
      {count > 0 && (
        <motion.div
          className="pointer-events-none fixed inset-x-0 bottom-[max(1.25rem,env(safe-area-inset-bottom))] z-10 flex justify-center px-4"
          initial={reduce ? { opacity: 0 } : { opacity: 0, y: 20, scale: 0.97 }}
          animate={reduce ? { opacity: 1 } : { opacity: 1, y: 0, scale: 1 }}
          exit={reduce ? { opacity: 0 } : { opacity: 0, y: 12, scale: 0.98 }}
          transition={reduce ? { duration: 0.12 } : { type: 'spring', stiffness: 420, damping: 34 }}
        >
          <div className="pointer-events-auto flex max-w-full items-center gap-1.5 overflow-x-auto rounded-full border border-line bg-raised py-1.5 pr-1.5 pl-4 whitespace-nowrap shadow-pop">
            {busy ? (
              <span className="flex items-center gap-2 pr-2 text-[13px] text-ink-2">
                <CircleNotch className="size-4 animate-spin text-accent" />
                {t('selection.working', { done: state.progress?.done ?? 0, total: state.progress?.total ?? count })}
              </span>
            ) : state.status === 'success' ? (
              <span className="pr-2 text-[13px] text-accent-strong">{state.message}</span>
            ) : state.status === 'error' ? (
              <span className="flex items-center gap-2 text-[13px] text-danger">
                <WarningCircle className="size-4 shrink-0" />
                {state.message}
                <Button variant="ghost" size="sm" onClick={onDismiss}>
                  {t('common:action.dismiss')}
                </Button>
              </span>
            ) : (
              <>
                <span className="text-[13px] text-ink-2">
                  <Trans
                    i18nKey="browse:selection.selected"
                    count={count}
                    components={{ num: <span className="font-mono text-ink" /> }}
                  />
                </span>
                <Button variant="ghost" size="sm" onClick={onSelectAll} disabled={count >= loaded}>
                  {t('selection.selectAll')}
                </Button>
                <Button variant="ghost" size="sm" onClick={onClear}>
                  {t('selection.clear')}
                </Button>
                <span className="mx-0.5 h-5 w-px shrink-0 bg-line" aria-hidden />
                {children}
              </>
            )}
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  )
}
