import { AnimatePresence, motion, useReducedMotion } from 'motion/react'
import { useTranslation } from 'react-i18next'
import {
  ArrowUUpLeft,
  ArrowUUpRight,
  BookOpen,
  CornersIn,
  CornersOut,
  DotsThreeVertical,
  Download,
  EyeSlash,
  ListBullets,
  SlidersHorizontal,
  X,
} from '@phosphor-icons/react'
import { IconButton } from '@/components/ui/IconButton'
import { Menu, MenuItem, MenuSeparator } from '@/components/ui/Menu'
import { Slider } from '@/components/ui/Slider'
import { Tooltip } from '@/components/ui/Tooltip'

interface EpubChromeProps {
  visible: boolean
  title: string
  position: number
  positionsCount: number
  incognito: boolean
  isFullscreen: boolean
  canDownloadFile: boolean
  hasPreviousBook: boolean
  hasNextBook: boolean
  onClose: () => void
  onGoToPosition: (position: number) => void
  onPreviousBook: () => void
  onNextBook: () => void
  onToggleToc: () => void
  onToggleSettings: () => void
  onToggleFullscreen: () => void
  onDownload: () => void
  onGoToBook: () => void
}

const chromeButton = 'text-white/85 hover:bg-white/10 hover:text-white'

export function EpubChrome({
  visible,
  title,
  position,
  positionsCount,
  incognito,
  isFullscreen,
  canDownloadFile,
  hasPreviousBook,
  hasNextBook,
  onClose,
  onGoToPosition,
  onPreviousBook,
  onNextBook,
  onToggleToc,
  onToggleSettings,
  onToggleFullscreen,
  onDownload,
  onGoToBook,
}: EpubChromeProps) {
  const reduceMotion = useReducedMotion()
  const { t } = useTranslation('reader')
  const transition = { duration: reduceMotion ? 0 : 0.25, ease: [0.16, 1, 0.3, 1] as const }

  return (
    <>
      <AnimatePresence>
        {visible && (
          <motion.div
            key="top"
            initial={{ y: '-100%', opacity: 0 }}
            animate={{ y: 0, opacity: 1 }}
            exit={{ y: '-100%', opacity: 0 }}
            transition={transition}
            className="fixed inset-x-0 top-0 z-20 bg-gradient-to-b from-black/70 to-transparent"
          >
            <div className="flex items-center gap-0.5 px-2 pt-[max(0.5rem,env(safe-area-inset-top))] pb-10 text-white">
              <IconButton label={t('chrome.closeReader')} className={chromeButton} onClick={onClose}>
                <X className="size-5" />
              </IconButton>
              <div className="flex min-w-0 flex-1 justify-center px-2">
                <span className="max-w-full truncate rounded-full bg-black/40 px-3 py-1 text-[13px] text-white/90">
                  {title}
                </span>
              </div>
              {incognito && (
                <Tooltip content={t('chrome.incognito')} side="bottom">
                  <span className="inline-flex size-9 shrink-0 items-center justify-center text-white/70">
                    <EyeSlash className="size-5" />
                  </span>
                </Tooltip>
              )}
              <IconButton label={t('epub.toc')} className={chromeButton} onClick={onToggleToc}>
                <ListBullets className="size-5" />
              </IconButton>
              <IconButton label={t('chrome.settings')} className={chromeButton} onClick={onToggleSettings}>
                <SlidersHorizontal className="size-5" />
              </IconButton>
              <IconButton
                label={isFullscreen ? t('chrome.exitFullscreen') : t('chrome.fullscreen')}
                className={chromeButton}
                onClick={onToggleFullscreen}
              >
                {isFullscreen ? <CornersIn className="size-5" /> : <CornersOut className="size-5" />}
              </IconButton>
              <Menu
                trigger={
                  <IconButton label={t('chrome.more')} className={chromeButton}>
                    <DotsThreeVertical className="size-5" />
                  </IconButton>
                }
              >
                {canDownloadFile && (
                  <MenuItem onSelect={onDownload}>
                    <Download className="size-4" /> {t('chrome.downloadFile')}
                  </MenuItem>
                )}
                {canDownloadFile && <MenuSeparator />}
                <MenuItem onSelect={onGoToBook}>
                  <BookOpen className="size-4" /> {t('chrome.goToBook')}
                </MenuItem>
              </Menu>
            </div>
          </motion.div>
        )}
        {visible && (
          <motion.div
            key="bottom"
            initial={{ y: '100%', opacity: 0 }}
            animate={{ y: 0, opacity: 1 }}
            exit={{ y: '100%', opacity: 0 }}
            transition={transition}
            className="fixed inset-x-0 bottom-0 z-20 bg-gradient-to-t from-black/70 to-transparent"
          >
            <div className="flex items-center gap-1.5 px-3 pt-10 pb-[max(0.75rem,env(safe-area-inset-bottom))] text-white">
              <IconButton
                label={t('chrome.previousBook')}
                className={chromeButton}
                disabled={!hasPreviousBook}
                onClick={onPreviousBook}
              >
                <ArrowUUpLeft className="size-5" />
              </IconButton>
              <div className="min-w-0 flex-1 px-2">
                <Slider
                  value={position}
                  onValueChange={onGoToPosition}
                  min={1}
                  max={Math.max(positionsCount, 1)}
                  label={t('epub.position')}
                />
              </div>
              <span className="shrink-0 font-mono text-xs text-white/80 tabular-nums" dir="ltr">
                {position} / {positionsCount}
              </span>
              <IconButton label={t('chrome.nextBook')} className={chromeButton} disabled={!hasNextBook} onClick={onNextBook}>
                <ArrowUUpRight className="size-5" />
              </IconButton>
            </div>
          </motion.div>
        )}
      </AnimatePresence>
      {!visible && positionsCount > 0 && (
        <div className="fixed inset-x-0 bottom-[env(safe-area-inset-bottom)] z-20 h-0.5 bg-white/10">
          <div className="h-full bg-accent" style={{ width: `${(position / positionsCount) * 100}%` }} />
        </div>
      )}
    </>
  )
}
