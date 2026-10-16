import { AnimatePresence, motion, useReducedMotion } from 'motion/react'
import { useTranslation } from 'react-i18next'
import { X } from '@phosphor-icons/react'
import type { ReadingDirection } from '@/lib/api/types'
import {
  READER_BACKGROUNDS,
  useReaderSettings,
  type ContinuousScaleType,
  type ReaderBackground,
  type ScaleType,
} from '@/lib/store/readerSettings'
import type { PagedReaderLayout } from '@/lib/utils/spreads'
import { IconButton } from '@/components/ui/IconButton'
import { SegmentedControl } from '@/components/ui/SegmentedControl'
import { Slider } from '@/components/ui/Slider'
import { Switch } from '@/components/ui/Switch'
import { Row, Section } from '@/components/reader/panel'
import { cn } from '@/lib/utils/cn'

const SCALE_LABEL_KEYS: Record<ScaleType, string> = {
  SCREEN: 'scale.screen',
  WIDTH: 'scale.width',
  WIDTH_SHRINK_ONLY: 'scale.shrink',
  HEIGHT: 'scale.height',
  ORIGINAL: 'scale.original',
}

const LAYOUT_LABEL_KEYS: Record<PagedReaderLayout, string> = {
  SINGLE_PAGE: 'layout.single',
  DOUBLE_PAGES: 'layout.double',
  DOUBLE_NO_COVER: 'layout.doubleNoCover',
}

const DIRECTION_LABEL_KEYS: Record<ReadingDirection, string> = {
  LEFT_TO_RIGHT: 'common:readingDirection.leftToRight',
  RIGHT_TO_LEFT: 'common:readingDirection.rightToLeft',
  VERTICAL: 'common:readingDirection.vertical',
  WEBTOON: 'common:readingDirection.webtoon',
}

const BACKGROUND_LABEL_KEYS: Record<ReaderBackground, string> = {
  BLACK: 'background.black',
  GRAY: 'background.gray',
  WHITE: 'background.white',
}

const PAGED_SCALE_VALUES: ScaleType[] = ['SCREEN', 'WIDTH', 'WIDTH_SHRINK_ONLY', 'HEIGHT', 'ORIGINAL']
const CONTINUOUS_SCALE_VALUES: ContinuousScaleType[] = ['WIDTH', 'ORIGINAL']
const LAYOUT_VALUES: PagedReaderLayout[] = ['SINGLE_PAGE', 'DOUBLE_PAGES', 'DOUBLE_NO_COVER']
const BACKGROUND_VALUES: ReaderBackground[] = ['BLACK', 'GRAY', 'WHITE']

interface SettingsPanelProps {
  open: boolean
  direction: ReadingDirection
  onDirectionChange: (direction: ReadingDirection) => void
  onAlwaysFullscreenChange: (on: boolean) => void
  onClose: () => void
}

export function SettingsPanel({
  open,
  direction,
  onDirectionChange,
  onAlwaysFullscreenChange,
  onClose,
}: SettingsPanelProps) {
  const scale = useReaderSettings((s) => s.scale)
  const pageLayout = useReaderSettings((s) => s.pageLayout)
  const continuousScale = useReaderSettings((s) => s.continuousScale)
  const continuousPadding = useReaderSettings((s) => s.continuousPadding)
  const continuousMargin = useReaderSettings((s) => s.continuousMargin)
  const swipe = useReaderSettings((s) => s.swipe)
  const animations = useReaderSettings((s) => s.animations)
  const alwaysFullscreen = useReaderSettings((s) => s.alwaysFullscreen)
  const background = useReaderSettings((s) => s.background)
  const update = useReaderSettings((s) => s.update)
  const reduceMotion = useReducedMotion()
  const { t } = useTranslation('reader')

  const webtoon = direction === 'WEBTOON'

  const directionOptions = (Object.keys(DIRECTION_LABEL_KEYS) as ReadingDirection[]).map((value) => ({
    value,
    label: t(DIRECTION_LABEL_KEYS[value]),
  }))
  const pagedScaleOptions = PAGED_SCALE_VALUES.map((value) => ({ value, label: t(SCALE_LABEL_KEYS[value]) }))
  const continuousScaleOptions = CONTINUOUS_SCALE_VALUES.map((value) => ({
    value,
    label: t(SCALE_LABEL_KEYS[value]),
  }))
  const layoutOptions = LAYOUT_VALUES.map((value) => ({ value, label: t(LAYOUT_LABEL_KEYS[value]) }))

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
              <Section title={t('panel.direction')}>
                <SegmentedControl
                  options={directionOptions}
                  value={direction}
                  onChange={onDirectionChange}
                  size="sm"
                  className="flex flex-wrap"
                />
              </Section>

              <Section title={t('panel.scale')}>
                {webtoon ? (
                  <SegmentedControl
                    options={continuousScaleOptions}
                    value={continuousScale}
                    onChange={(v) => update({ continuousScale: v })}
                    size="sm"
                  />
                ) : (
                  <SegmentedControl
                    options={pagedScaleOptions}
                    value={scale}
                    onChange={(v) => update({ scale: v })}
                    size="sm"
                    className="flex flex-wrap"
                  />
                )}
              </Section>

              {!webtoon && (
                <Section title={t('panel.layout')}>
                  <SegmentedControl
                    options={layoutOptions}
                    value={pageLayout}
                    onChange={(v) => update({ pageLayout: v })}
                    size="sm"
                  />
                </Section>
              )}

              <Section title={t('panel.background')}>
                <div className="flex gap-4">
                  {BACKGROUND_VALUES.map((value) => (
                    <button
                      key={value}
                      type="button"
                      onClick={() => update({ background: value })}
                      className="flex cursor-pointer flex-col items-center gap-1.5 text-[11px] text-white/60"
                    >
                      <span
                        className={cn(
                          'size-8 rounded-full border',
                          background === value ? 'border-accent ring-2 ring-accent/40' : 'border-white/25',
                        )}
                        style={{ background: READER_BACKGROUNDS[value] }}
                      />
                      {t(BACKGROUND_LABEL_KEYS[value])}
                    </button>
                  ))}
                </div>
              </Section>

              <Section title={t('panel.behavior')}>
                <div className="space-y-3">
                  <Row label={t('panel.swipe')}>
                    <Switch checked={swipe} onCheckedChange={(v) => update({ swipe: v })} label={t('panel.swipe')} />
                  </Row>
                  <Row label={t('panel.animations')}>
                    <Switch
                      checked={animations}
                      onCheckedChange={(v) => update({ animations: v })}
                      label={t('panel.animations')}
                    />
                  </Row>
                  <Row label={t('panel.alwaysFullscreen')}>
                    <Switch
                      checked={alwaysFullscreen}
                      onCheckedChange={onAlwaysFullscreenChange}
                      label={t('panel.alwaysFullscreen')}
                    />
                  </Row>
                </div>
              </Section>

              {webtoon && (
                <Section title={t('common:readingDirection.webtoon')}>
                  <div className="space-y-4">
                    <div className="space-y-1.5">
                      <div className="flex items-center justify-between text-[13px] text-white/80">
                        <span>{t('panel.sidePadding')}</span>
                        <span className="font-mono text-xs text-white/50">{continuousPadding}%</span>
                      </div>
                      <Slider
                        value={continuousPadding}
                        onValueChange={(v) => update({ continuousPadding: v })}
                        min={0}
                        max={40}
                        step={5}
                        label={t('panel.sidePadding')}
                      />
                    </div>
                    <div className="space-y-1.5">
                      <div className="flex items-center justify-between text-[13px] text-white/80">
                        <span>{t('panel.pageGap')}</span>
                        <span className="font-mono text-xs text-white/50">{continuousMargin}px</span>
                      </div>
                      <Slider
                        value={continuousMargin}
                        onValueChange={(v) => update({ continuousMargin: v })}
                        min={0}
                        max={15}
                        step={5}
                        label={t('panel.pageGap')}
                      />
                    </div>
                  </div>
                </Section>
              )}
            </div>
          </motion.aside>
        </>
      )}
    </AnimatePresence>
  )
}
