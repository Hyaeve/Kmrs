import { AnimatePresence, motion, useReducedMotion } from 'motion/react'
import { useTranslation } from 'react-i18next'
import {
  ArrowUUpLeft,
  ArrowUUpRight,
  Book,
  BookOpen,
  Books,
  CornersIn,
  CornersOut,
  DotsThreeVertical,
  Download,
  EyeSlash,
  FileImage,
  ListBullets,
  Question,
  SkipBack,
  SkipForward,
  SlidersHorizontal,
  SquaresFour,
  X,
} from '@phosphor-icons/react'
import { IconButton } from '@/components/ui/IconButton'
import { Menu, MenuItem, MenuLabel, MenuSeparator } from '@/components/ui/Menu'
import { Slider } from '@/components/ui/Slider'
import { Tooltip } from '@/components/ui/Tooltip'

export type PosterTarget = 'book' | 'series' | 'readlist'

interface ReaderChromeProps {
  visible: boolean
  title: string
  page: number
  pagesCount: number
  rtl: boolean
  incognito: boolean
  isFullscreen: boolean
  canDownloadFile: boolean
  canSetPoster: boolean
  readListContext: boolean
  posterBusy: boolean
  hasPreviousBook: boolean
  hasNextBook: boolean
  onClose: () => void
  onGoToPage: (page: number) => void
  onFirstPage: () => void
  onLastPage: () => void
  onPreviousBook: () => void
  onNextBook: () => void
  onToggleExplorer: () => void
  onToggleSettings: () => void
  onToggleHelp: () => void
  onToggleFullscreen: () => void
  onDownload: () => void
  onDownloadPage: () => void
  onSetPoster: (target: PosterTarget) => void
  onGoToBook: () => void
}

const chromeButton = 'text-white/85 hover:bg-white/10 hover:text-white'

export function ReaderChrome({
  visible,
  title,
  page,
  pagesCount,
  rtl,
  incognito,
  isFullscreen,
  canDownloadFile,
  canSetPoster,
  readListContext,
  posterBusy,
  hasPreviousBook,
  hasNextBook,
  onClose,
  onGoToPage,
  onFirstPage,
  onLastPage,
  onPreviousBook,
  onNextBook,
  onToggleExplorer,
  onToggleSettings,
  onToggleHelp,
  onToggleFullscreen,
  onDownload,
  onDownloadPage,
  onSetPoster,
  onGoToBook,
}: ReaderChromeProps) {
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
              <IconButton label={t('chrome.pages')} className={chromeButton} onClick={onToggleExplorer}>
                <SquaresFour className="size-5" />
              </IconButton>
              <IconButton label={t('chrome.settings')} className={chromeButton} onClick={onToggleSettings}>
                <SlidersHorizontal className="size-5" />
              </IconButton>
              <IconButton label={t('shortcuts.title')} className={chromeButton} onClick={onToggleHelp}>
                <Question className="size-5" />
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
                {canDownloadFile && (
                  <MenuItem onSelect={onDownloadPage}>
                    <FileImage className="size-4" /> {t('chrome.downloadPage', { page })}
                  </MenuItem>
                )}
                {canSetPoster && (
                  <>
                    <MenuSeparator />
                    <MenuLabel>{t('chrome.setPoster', { page })}</MenuLabel>
                    <MenuItem disabled={posterBusy} onSelect={() => onSetPoster('book')}>
                      <Book className="size-4" /> {t('chrome.bookPoster')}
                    </MenuItem>
                    <MenuItem disabled={posterBusy} onSelect={() => onSetPoster('series')}>
                      <Books className="size-4" /> {t('chrome.seriesPoster')}
                    </MenuItem>
                    {readListContext && (
                      <MenuItem disabled={posterBusy} onSelect={() => onSetPoster('readlist')}>
                        <ListBullets className="size-4" /> {t('chrome.readListPoster')}
                      </MenuItem>
                    )}
                  </>
                )}
                <MenuSeparator />
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
            <div dir={rtl ? 'rtl' : 'ltr'} className="flex items-center gap-1.5 px-3 pt-10 pb-[max(0.75rem,env(safe-area-inset-bottom))] text-white">
              <IconButton
                label={t('chrome.previousBook')}
                className={chromeButton}
                disabled={!hasPreviousBook}
                onClick={onPreviousBook}
              >
                <ArrowUUpLeft className="size-5" />
              </IconButton>
              <IconButton label={t('chrome.firstPage')} className={chromeButton} onClick={onFirstPage}>
                <SkipBack className="size-5" />
              </IconButton>
              <div className="min-w-0 flex-1 px-2">
                <Slider value={page} onValueChange={onGoToPage} min={1} max={pagesCount} label={t('chrome.page')} dir={rtl ? 'rtl' : 'ltr'} />
              </div>
              <span className="shrink-0 font-mono text-xs text-white/80 tabular-nums" dir="ltr">
                {page} / {pagesCount}
              </span>
              <IconButton label={t('chrome.lastPage')} className={chromeButton} onClick={onLastPage}>
                <SkipForward className="size-5" />
              </IconButton>
              <IconButton label={t('chrome.nextBook')} className={chromeButton} disabled={!hasNextBook} onClick={onNextBook}>
                <ArrowUUpRight className="size-5" />
              </IconButton>
            </div>
          </motion.div>
        )}
      </AnimatePresence>
      {!visible && (
        <div dir={rtl ? 'rtl' : 'ltr'} className="fixed inset-x-0 bottom-[env(safe-area-inset-bottom)] z-20 h-0.5 bg-white/10">
          <div className="h-full bg-accent" style={{ width: `${(page / pagesCount) * 100}%` }} />
        </div>
      )}
    </>
  )
}
