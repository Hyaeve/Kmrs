import { AnimatePresence, motion, useReducedMotion } from 'motion/react'
import { useTranslation } from 'react-i18next'
import { useQuery } from '@tanstack/react-query'
import { ArrowCounterClockwise, X } from '@phosphor-icons/react'
import { useReaderSettings, type EpubTheme } from '@/lib/store/readerSettings'
import { fontsApi } from '@/lib/api/fonts'
import { IconButton } from '@/components/ui/IconButton'
import { SegmentedControl } from '@/components/ui/SegmentedControl'
import { Slider } from '@/components/ui/Slider'
import { Switch } from '@/components/ui/Switch'
import { Row, Section } from '@/components/reader/panel'
import { isServerFont, serverFontValue } from '@/components/reader/epub/fonts'
import { cn } from '@/lib/utils/cn'

const THEME_VALUES: EpubTheme[] = ['DAY', 'SEPIA', 'NIGHT']
const THEME_LABEL_KEYS: Record<EpubTheme, string> = {
  DAY: 'epub.themeDay',
  SEPIA: 'epub.themeSepia',
  NIGHT: 'epub.themeNight',
}
const THEME_SWATCH: Record<EpubTheme, { bg: string; fg: string }> = {
  DAY: { bg: '#fefefe', fg: '#121212' },
  SEPIA: { bg: '#e9ddc8', fg: '#000000' },
  NIGHT: { bg: '#000000', fg: '#fefefe' },
}

interface EpubSettingsPanelProps {
  open: boolean
  onAlwaysFullscreenChange: (on: boolean) => void
  onClose: () => void
}

export function EpubSettingsPanel({ open, onAlwaysFullscreenChange, onClose }: EpubSettingsPanelProps) {
  const theme = useReaderSettings((s) => s.epubTheme)
  const scroll = useReaderSettings((s) => s.epubScroll)
  const fontFamily = useReaderSettings((s) => s.epubFontFamily)
  const fontSize = useReaderSettings((s) => s.epubFontSize)
  const lineHeight = useReaderSettings((s) => s.epubLineHeight)
  const alwaysFullscreen = useReaderSettings((s) => s.alwaysFullscreen)
  const update = useReaderSettings((s) => s.update)
  const reduceMotion = useReducedMotion()
  const { t } = useTranslation('reader')
  const serverFontsQuery = useQuery({ queryKey: ['fonts', 'families'], queryFn: fontsApi.families, staleTime: Infinity })
  const serverFonts = serverFontsQuery.data ?? []

  const layoutOptions = [
    { value: 'paginated', label: t('epub.layoutPaginated') },
    { value: 'scroll', label: t('epub.layoutScroll') },
  ]

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
            aria-label={t('panel.title')}
            initial={{ x: '100%' }}
            animate={{ x: 0 }}
            exit={{ x: '100%' }}
            transition={{ duration: reduceMotion ? 0 : 0.25, ease: [0.16, 1, 0.3, 1] }}
            className="fixed top-0 right-0 z-30 flex h-full w-80 max-w-[85vw] safe-top safe-bottom flex-col border-l border-white/10 bg-black/85 text-white backdrop-blur-md"
          >
            <div className="flex shrink-0 items-center justify-between border-b border-white/10 px-4 py-3">
              <h2 className="text-[15px] font-semibold">{t('panel.title')}</h2>
              <IconButton
                label={t('panel.close')}
                className="text-white/80 hover:bg-white/10 hover:text-white"
                onClick={onClose}
              >
                <X className="size-4" />
              </IconButton>
            </div>

            <div className="min-h-0 flex-1 space-y-6 overflow-y-auto px-4 py-4">
              <Section title={t('epub.theme')}>
                <div className="flex gap-4">
                  {THEME_VALUES.map((value) => (
                    <button
                      key={value}
                      type="button"
                      onClick={() => update({ epubTheme: value })}
                      className="flex cursor-pointer flex-col items-center gap-1.5 text-[11px] text-white/60"
                    >
                      <span
                        className={cn(
                          'flex size-8 items-center justify-center rounded-full border text-[13px] font-medium',
                          theme === value ? 'border-accent ring-2 ring-accent/40' : 'border-white/25',
                        )}
                        style={{ background: THEME_SWATCH[value].bg, color: THEME_SWATCH[value].fg }}
                      >
                        A
                      </span>
                      {t(THEME_LABEL_KEYS[value])}
                    </button>
                  ))}
                </div>
              </Section>

              <Section title={t('epub.layout')}>
                <SegmentedControl
                  options={layoutOptions}
                  value={scroll ? 'scroll' : 'paginated'}
                  onChange={(v) => update({ epubScroll: v === 'scroll' })}
                  size="sm"
                  className="flex flex-wrap"
                />
              </Section>

              <Section title={t('epub.typography')}>
                <div className="space-y-4">
                  <div className="space-y-1.5">
                    <div className="text-[13px] text-white/80">{t('epub.fontFamily')}</div>
                    <div className="flex flex-wrap gap-1.5">
                      <button
                        type="button"
                        onClick={() => update({ epubFontFamily: 'Original' })}
                        className={cn(
                          'cursor-pointer rounded-md border px-2.5 py-1 text-[13px] transition-colors',
                          fontFamily === 'Original'
                            ? 'border-accent text-white'
                            : 'border-white/20 text-white/60 hover:border-white/40 hover:text-white/80',
                        )}
                      >
                        {t('epub.fontPublisher')}
                      </button>
                      {serverFonts.map((family) => (
                        <button
                          key={family}
                          type="button"
                          onClick={() => update({ epubFontFamily: serverFontValue(family) })}
                          className={cn(
                            'cursor-pointer rounded-md border px-2.5 py-1 text-[13px] transition-colors',
                            isServerFont(fontFamily) && fontFamily === serverFontValue(family)
                              ? 'border-accent text-white'
                              : 'border-white/20 text-white/60 hover:border-white/40 hover:text-white/80',
                          )}
                        >
                          {family}
                        </button>
                      ))}
                    </div>
                  </div>
                  <div className="space-y-1.5">
                    <div className="flex items-center justify-between text-[13px] text-white/80">
                      <span className="flex items-center gap-1.5">
                        {t('epub.fontSize')}
                        {fontSize !== 1 && (
                          <IconButton
                            label={t('epub.resetDefault')}
                            className="size-6 text-white/50 hover:bg-white/10 hover:text-white"
                            onClick={() => update({ epubFontSize: 1 })}
                          >
                            <ArrowCounterClockwise className="size-3.5" />
                          </IconButton>
                        )}
                      </span>
                      <span className="font-mono text-xs text-white/50">{Math.round(fontSize * 100)}%</span>
                    </div>
                    <Slider
                      value={fontSize}
                      onValueChange={(v) => update({ epubFontSize: v })}
                      min={0.7}
                      max={2}
                      step={0.05}
                      label={t('epub.fontSize')}
                    />
                  </div>
                  <div className="space-y-1.5">
                    <div className="flex items-center justify-between text-[13px] text-white/80">
                      <span className="flex items-center gap-1.5">
                        {t('epub.lineHeight')}
                        {lineHeight !== null && (
                          <IconButton
                            label={t('epub.resetDefault')}
                            className="size-6 text-white/50 hover:bg-white/10 hover:text-white"
                            onClick={() => update({ epubLineHeight: null })}
                          >
                            <ArrowCounterClockwise className="size-3.5" />
                          </IconButton>
                        )}
                      </span>
                      <span className="font-mono text-xs text-white/50">
                        {lineHeight === null ? t('epub.default') : lineHeight.toFixed(1)}
                      </span>
                    </div>
                    <Slider
                      value={lineHeight ?? 1.5}
                      onValueChange={(v) => update({ epubLineHeight: v })}
                      min={1}
                      max={2.5}
                      step={0.1}
                      label={t('epub.lineHeight')}
                    />
                  </div>
                </div>
              </Section>

              <Section title={t('panel.behavior')}>
                <Row label={t('panel.alwaysFullscreen')}>
                  <Switch
                    checked={alwaysFullscreen}
                    onCheckedChange={onAlwaysFullscreenChange}
                    label={t('panel.alwaysFullscreen')}
                  />
                </Row>
              </Section>
            </div>
          </motion.aside>
        </>
      )}
    </AnimatePresence>
  )
}
