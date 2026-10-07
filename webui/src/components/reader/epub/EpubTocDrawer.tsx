import { AnimatePresence, motion, useReducedMotion } from 'motion/react'
import { useTranslation } from 'react-i18next'
import { X } from '@phosphor-icons/react'
import { IconButton } from '@/components/ui/IconButton'
import type { TocEntry } from './preferences'
import { cn } from '@/lib/utils/cn'

interface EpubTocDrawerProps {
  open: boolean
  toc: TocEntry[]
  landmarks: TocEntry[]
  pageList: TocEntry[]
  currentHref?: string
  onGo: (href: string) => void
  onClose: () => void
}

function EntryList({ entries, depth, currentHref, onGo }: { entries: TocEntry[]; depth: number; currentHref?: string; onGo: (href: string) => void }) {
  return (
    <ul>
      {entries.map((entry, i) => {
        const hrefNoFragment = entry.href?.split('#')[0]
        const active = !!entry.href && currentHref === hrefNoFragment
        return (
          <li key={`${entry.href ?? entry.title ?? i}-${i}`}>
            <button
              type="button"
              disabled={!entry.href}
              onClick={() => entry.href && onGo(entry.href)}
              style={{ paddingLeft: `${depth * 14 + 12}px` }}
              className={cn(
                'w-full cursor-pointer truncate rounded-md py-1.5 pr-3 text-left text-[13px]',
                active ? 'bg-white/10 font-medium text-white' : 'text-white/75 hover:bg-white/5 hover:text-white',
                !entry.href && 'cursor-default text-white/45',
              )}
            >
              {entry.title || entry.href}
            </button>
            {entry.children && entry.children.length > 0 && (
              <EntryList entries={entry.children} depth={depth + 1} currentHref={currentHref} onGo={onGo} />
            )}
          </li>
        )
      })}
    </ul>
  )
}

export function EpubTocDrawer({ open, toc, landmarks, pageList, currentHref, onGo, onClose }: EpubTocDrawerProps) {
  const reduceMotion = useReducedMotion()
  const { t } = useTranslation('reader')

  const sections = [
    { key: 'toc', title: t('epub.toc'), entries: toc },
    { key: 'landmarks', title: t('epub.landmarks'), entries: landmarks },
    { key: 'pageList', title: t('epub.pageList'), entries: pageList },
  ].filter((s) => s.entries.length > 0)

  return (
    <AnimatePresence>
      {open && (
        <>
          <motion.div
            key="backdrop"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: reduceMotion ? 0 : 0.2 }}
            className="fixed inset-0 z-30 bg-black/40"
            onClick={onClose}
          />
          <motion.aside
            key="panel"
            role="dialog"
            aria-label={t('epub.toc')}
            initial={{ x: '-100%' }}
            animate={{ x: 0 }}
            exit={{ x: '-100%' }}
            transition={{ duration: reduceMotion ? 0 : 0.25, ease: [0.16, 1, 0.3, 1] }}
            className="fixed top-0 left-0 z-30 flex h-full w-80 max-w-[85vw] safe-top safe-bottom flex-col border-r border-white/10 bg-black/85 text-white backdrop-blur-md"
          >
            <div className="flex shrink-0 items-center justify-between border-b border-white/10 px-4 py-3">
              <h2 className="text-[15px] font-semibold">{t('epub.toc')}</h2>
              <IconButton
                label={t('panel.close')}
                className="text-white/80 hover:bg-white/10 hover:text-white"
                onClick={onClose}
              >
                <X className="size-4" />
              </IconButton>
            </div>
            <div className="min-h-0 flex-1 space-y-5 overflow-y-auto px-2 py-4">
              {sections.map((s) => (
                <section key={s.key}>
                  {sections.length > 1 && (
                    <h3 className="px-3 pb-1.5 text-[11px] font-medium tracking-wide text-white/50 uppercase">
                      {s.title}
                    </h3>
                  )}
                  <EntryList entries={s.entries} depth={0} currentHref={currentHref} onGo={onGo} />
                </section>
              ))}
            </div>
          </motion.aside>
        </>
      )}
    </AnimatePresence>
  )
}
