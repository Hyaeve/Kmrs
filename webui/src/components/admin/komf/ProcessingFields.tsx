import { useTranslation } from 'react-i18next'
import type {
  KomfChineseDirection,
  KomfChineseField,
  KomfLibraryType,
  KomfPublisherTagName,
  KomfReadingDirection,
  KomfUpdateMode,
} from '@/lib/api/types'
import type { ProcessingDraft } from './draft'
import { FieldInput } from '@/components/admin/settings/FieldInput'
import { IconButton } from '@/components/ui/IconButton'
import { Button } from '@/components/ui/Button'
import { SegmentedControl } from '@/components/ui/SegmentedControl'
import { Switch } from '@/components/ui/Switch'
import { Plus, X } from '@phosphor-icons/react'
import { Checklist, SelectInput, StringListInput, Subheading } from './fields'
import { FormRow } from './FormRow'

const LIBRARY_TYPE_OPTIONS: Array<{ value: KomfLibraryType; labelKey: string }> = [
  { value: 'MANGA', labelKey: 'libraryType.manga' },
  { value: 'NOVEL', labelKey: 'libraryType.novel' },
  { value: 'COMIC', labelKey: 'libraryType.comic' },
  { value: 'WEBTOON', labelKey: 'libraryType.webtoon' },
]

const UPDATE_MODE_OPTIONS: Array<{ value: KomfUpdateMode; label: string }> = [
  { value: 'API', label: 'API' },
  { value: 'COMIC_INFO', label: 'ComicInfo' },
  { value: 'MYLAR_SERIES_JSON', label: 'Mylar series.json' },
]

const READING_DIRECTION_OPTIONS: Array<{ value: KomfReadingDirection | ''; labelKey: string }> = [
  { value: '', labelKey: 'noOverride' },
  { value: 'LEFT_TO_RIGHT', labelKey: 'common:readingDirection.leftToRight' },
  { value: 'RIGHT_TO_LEFT', labelKey: 'common:readingDirection.rightToLeft' },
  { value: 'VERTICAL', labelKey: 'common:readingDirection.vertical' },
  { value: 'WEBTOON', labelKey: 'common:readingDirection.webtoon' },
]

const CHINESE_DIRECTION_OPTIONS: Array<{ value: KomfChineseDirection; labelKey: string }> = [
  { value: 't2s', labelKey: 'chinese.directionValue.t2s' },
  { value: 's2t', labelKey: 'chinese.directionValue.s2t' },
]

const CHINESE_FIELD_OPTIONS: Array<{ value: KomfChineseField; labelKey: string }> = [
  { value: 'title', labelKey: 'metadata:field.title' },
  { value: 'genres', labelKey: 'metadata:field.genres' },
  { value: 'tags', labelKey: 'metadata:field.tags' },
  { value: 'summary', labelKey: 'metadata:field.summary' },
]

interface ProcessingGroupProps {
  value: ProcessingDraft
  onChange: (v: ProcessingDraft) => void
}

// Each group renders a fragment so the composed ProcessingFields below keeps the
// same first:/last: FormRow spacing as a flat list; the tab view wraps each group
// in a plain div to restore that spacing inside its own card.

export function ProcessingGeneralFields({ value, onChange }: ProcessingGroupProps) {
  const { t } = useTranslation('admin-komf')
  const set = (patch: Partial<ProcessingDraft>) => onChange({ ...value, ...patch })

  const toggleMode = (mode: KomfUpdateMode, on: boolean) =>
    set({ updateModes: on ? [...value.updateModes, mode] : value.updateModes.filter((m) => m !== mode) })

  return (
    <>
      <FormRow label={t('processing.libraryType')}>
        <SegmentedControl
          options={LIBRARY_TYPE_OPTIONS.map((o) => ({ value: o.value, label: t(o.labelKey) }))}
          value={value.libraryType}
          onChange={(v) => set({ libraryType: v })}
        />
      </FormRow>
      <FormRow label={t('processing.updateModes')}>
        <div className="flex flex-col gap-1.5">
          {UPDATE_MODE_OPTIONS.map((opt) => (
            <label key={opt.value} className="flex cursor-pointer items-center gap-2 text-sm text-ink-2">
              <input
                type="checkbox"
                checked={value.updateModes.includes(opt.value)}
                onChange={(e) => toggleMode(opt.value, e.target.checked)}
                className="size-4 cursor-pointer accent-accent"
              />
              {opt.label}
            </label>
          ))}
        </div>
      </FormRow>
      <FormRow label={t('processing.aggregate')}>
        <Switch checked={value.aggregate} onCheckedChange={(v) => set({ aggregate: v })} label={t('processing.aggregate')} />
      </FormRow>
      <FormRow label={t('processing.mergeTags')}>
        <Switch checked={value.mergeTags} onCheckedChange={(v) => set({ mergeTags: v })} label={t('processing.mergeTags')} />
      </FormRow>
      <FormRow label={t('processing.mergeGenres')}>
        <Switch
          checked={value.mergeGenres}
          onCheckedChange={(v) => set({ mergeGenres: v })}
          label={t('processing.mergeGenres')}
        />
      </FormRow>
      <FormRow label={t('processing.bookCovers')}>
        <Switch
          checked={value.bookCovers}
          onCheckedChange={(v) => set({ bookCovers: v })}
          label={t('processing.bookCovers')}
        />
      </FormRow>
      <FormRow label={t('processing.seriesCovers')}>
        <Switch
          checked={value.seriesCovers}
          onCheckedChange={(v) => set({ seriesCovers: v })}
          label={t('processing.seriesCovers')}
        />
      </FormRow>
      <FormRow label={t('processing.overrideExistingCovers')}>
        <Switch
          checked={value.overrideExistingCovers}
          onCheckedChange={(v) => set({ overrideExistingCovers: v })}
          label={t('processing.overrideExistingCovers')}
        />
      </FormRow>
      <FormRow label={t('processing.lockCovers')}>
        <Switch
          checked={value.lockCovers}
          onCheckedChange={(v) => set({ lockCovers: v })}
          label={t('processing.lockCovers')}
        />
      </FormRow>
      <FormRow label={t('processing.overrideComicInfo')}>
        <Switch
          checked={value.overrideComicInfo}
          onCheckedChange={(v) => set({ overrideComicInfo: v })}
          label={t('processing.overrideComicInfo')}
        />
      </FormRow>
      <FormRow label={t('processing.failedMatchCollection')} helper={t('processing.failedMatchCollectionHelper')}>
        <FieldInput
          aria-label={t('processing.failedMatchCollection')}
          className="w-56"
          value={value.failedMatchCollectionName}
          onChange={(e) => set({ failedMatchCollectionName: e.target.value })}
        />
      </FormRow>
    </>
  )
}

export function PostProcessingFields({ value, onChange }: ProcessingGroupProps) {
  const { t } = useTranslation('admin-komf')
  const set = (patch: Partial<ProcessingDraft>) => onChange({ ...value, ...patch })
  const setLabels = (patch: Partial<ProcessingDraft['alternateTitleLabels']>) =>
    set({ alternateTitleLabels: { ...value.alternateTitleLabels, ...patch } })

  const setPublisherTagName = (index: number, patch: Partial<KomfPublisherTagName>) =>
    set({
      publisherTagNames: value.publisherTagNames.map((tag, i) => (i === index ? { ...tag, ...patch } : tag)),
    })

  return (
    <>
      <FormRow label={t('processing.seriesTitle')}>
        <Switch
          checked={value.seriesTitle}
          onCheckedChange={(v) => set({ seriesTitle: v })}
          label={t('processing.seriesTitle')}
        />
      </FormRow>
      <FormRow label={t('processing.seriesTitleLanguage')}>
        <FieldInput
          aria-label={t('processing.seriesTitleLanguage')}
          className="w-36"
          value={value.seriesTitleLanguage}
          onChange={(e) => set({ seriesTitleLanguage: e.target.value })}
        />
      </FormRow>
      <FormRow label={t('processing.alternativeSeriesTitles')}>
        <Switch
          checked={value.alternativeSeriesTitles}
          onCheckedChange={(v) => set({ alternativeSeriesTitles: v })}
          label={t('processing.alternativeSeriesTitles')}
        />
      </FormRow>
      <div className="py-3">
        <p className="text-sm text-ink-2">{t('processing.alternativeTitleLanguages')}</p>
        <p className="mt-0.5 text-xs text-ink-3">{t('processing.bcp47Helper')}</p>
        <div className="mt-2">
          <StringListInput
            aria-label={t('processing.alternativeTitleLanguages')}
            value={value.alternativeSeriesTitleLanguages}
            onChange={(v) => set({ alternativeSeriesTitleLanguages: v })}
          />
        </div>
      </div>
      <FormRow label={t('processing.fallbackToAltTitle')}>
        <Switch
          checked={value.fallbackToAltTitle}
          onCheckedChange={(v) => set({ fallbackToAltTitle: v })}
          label={t('processing.fallbackToAltTitle')}
        />
      </FormRow>
      <FormRow label={t('processing.orderBooks')}>
        <Switch
          checked={value.orderBooks}
          onCheckedChange={(v) => set({ orderBooks: v })}
          label={t('processing.orderBooks')}
        />
      </FormRow>
      <FormRow label={t('metadata:field.readingDirection')}>
        <SelectInput
          aria-label={t('metadata:field.readingDirection')}
          options={READING_DIRECTION_OPTIONS.map((o) => ({ value: o.value, label: t(o.labelKey) }))}
          value={value.readingDirectionValue}
          onChange={(v) => set({ readingDirectionValue: v })}
        />
      </FormRow>
      <FormRow label={t('metadata:field.language')}>
        <FieldInput
          aria-label={t('metadata:field.language')}
          className="w-36"
          value={value.languageValue}
          onChange={(e) => set({ languageValue: e.target.value })}
        />
      </FormRow>
      <FormRow label={t('processing.scoreTagName')} helper={t('processing.scoreTagNameHelper')}>
        <FieldInput
          aria-label={t('processing.scoreTagName')}
          className="w-36"
          placeholder="score:"
          value={value.scoreTagName}
          onChange={(e) => set({ scoreTagName: e.target.value })}
        />
      </FormRow>
      <FormRow label={t('processing.originalPublisherTagName')}>
        <FieldInput
          aria-label={t('processing.originalPublisherTagName')}
          className="w-36"
          value={value.originalPublisherTagName}
          onChange={(e) => set({ originalPublisherTagName: e.target.value })}
        />
      </FormRow>
      <div className="py-3">
        <p className="text-sm text-ink-2">{t('processing.publisherTagNames')}</p>
        <p className="mt-0.5 text-xs text-ink-3">{t('processing.publisherTagNamesHelper')}</p>
        <div className="mt-2 flex flex-col gap-1.5">
          {value.publisherTagNames.map((tag, i) => (
            <div key={i} className="flex items-center gap-1.5">
              <FieldInput
                aria-label={t('processing.publisherTagName', { index: i + 1 })}
                className="w-40"
                placeholder={t('processing.tagNamePlaceholder')}
                value={tag.tagName}
                onChange={(e) => setPublisherTagName(i, { tagName: e.target.value })}
              />
              <FieldInput
                aria-label={t('processing.publisherTagLanguage', { index: i + 1 })}
                className="w-28"
                placeholder={t('metadata:field.language')}
                value={tag.language}
                onChange={(e) => setPublisherTagName(i, { language: e.target.value })}
              />
              <IconButton
                label={t('processing.removePublisherTag')}
                className="size-8"
                onClick={() => set({ publisherTagNames: value.publisherTagNames.filter((_, j) => j !== i) })}
              >
                <X className="size-4" />
              </IconButton>
            </div>
          ))}
          <div>
            <Button
              size="sm"
              variant="ghost"
              onClick={() => set({ publisherTagNames: [...value.publisherTagNames, { tagName: '', language: '' }] })}
            >
              <Plus className="size-4" /> {t('processing.addTagName')}
            </Button>
          </div>
        </div>
      </div>
      <div className="py-3">
        <p className="text-sm text-ink-2">{t('processing.alternateTitleLabels')}</p>
        <p className="mt-0.5 text-xs text-ink-3">{t('processing.alternateTitleLabelsHelper')}</p>
        <div className="mt-2 flex flex-wrap gap-1.5">
          <FieldInput
            aria-label={t('processing.romajiLabel')}
            className="w-36"
            placeholder={t('processing.romajiPlaceholder')}
            value={value.alternateTitleLabels.romaji}
            onChange={(e) => setLabels({ romaji: e.target.value })}
          />
          <FieldInput
            aria-label={t('processing.nativeLabel')}
            className="w-36"
            placeholder={t('processing.nativePlaceholder')}
            value={value.alternateTitleLabels.native}
            onChange={(e) => setLabels({ native: e.target.value })}
          />
          <FieldInput
            aria-label={t('processing.localizedLabel')}
            className="w-36"
            placeholder={t('processing.localizedPlaceholder')}
            value={value.alternateTitleLabels.localized}
            onChange={(e) => setLabels({ localized: e.target.value })}
          />
        </div>
      </div>
      <FormRow label={t('processing.skipWithLinks')} helper={t('processing.skipWithLinksHelper')}>
        <Switch
          checked={value.linksSkipEnabled}
          onCheckedChange={(v) => set({ linksSkipEnabled: v })}
          label={t('processing.skipWithLinks')}
        />
      </FormRow>
      <FormRow label={t('processing.matchByLinks')} helper={t('processing.matchByLinksHelper')}>
        <Switch
          checked={value.linksMatchEnabled}
          onCheckedChange={(v) => set({ linksMatchEnabled: v })}
          label={t('processing.matchByLinks')}
        />
      </FormRow>
    </>
  )
}

export function SearchTitleExtractionFields({ value, onChange }: ProcessingGroupProps) {
  const { t } = useTranslation('admin-komf')
  const set = (patch: Partial<ProcessingDraft>) => onChange({ ...value, ...patch })
  const setExtraction = (patch: Partial<ProcessingDraft['searchTitleExtraction']>) =>
    set({ searchTitleExtraction: { ...value.searchTitleExtraction, ...patch } })

  const setCharMapping = (index: number, patch: Partial<{ from: string; to: string }>) =>
    setExtraction({
      charMappings: value.searchTitleExtraction.charMappings.map((m, i) => (i === index ? { ...m, ...patch } : m)),
    })

  return (
    <>
      <FormRow label={t('enabled')} helper={t('extraction.enabledHelper')}>
        <Switch
          checked={value.searchTitleExtraction.enabled}
          onCheckedChange={(v) => setExtraction({ enabled: v })}
          label={t('section.searchTitleExtraction')}
        />
      </FormRow>
      {value.searchTitleExtraction.enabled && (
        <>
          <FormRow label={t('extraction.bracketRegex')} helper={t('extraction.bracketRegexHelper')}>
            <FieldInput
              aria-label={t('extraction.bracketRegex')}
              className="w-56 font-mono text-[13px]"
              value={value.searchTitleExtraction.bracketRegex}
              onChange={(e) => setExtraction({ bracketRegex: e.target.value })}
            />
          </FormRow>
          <FormRow label={t('extraction.authorSeparator')}>
            <FieldInput
              aria-label={t('extraction.authorSeparator')}
              className="w-36"
              value={value.searchTitleExtraction.authorSeparator}
              onChange={(e) => setExtraction({ authorSeparator: e.target.value })}
            />
          </FormRow>
          <div className="py-3">
            <p className="text-sm text-ink-2">{t('extraction.titleSplitters')}</p>
            <p className="mt-0.5 text-xs text-ink-3">{t('extraction.titleSplittersHelper')}</p>
            <div className="mt-2">
              <StringListInput
                aria-label={t('extraction.titleSplitters')}
                value={value.searchTitleExtraction.titleSplitters}
                onChange={(v) => setExtraction({ titleSplitters: v })}
              />
            </div>
          </div>
          <FormRow label={t('extraction.symbolNormalizeRegex')} helper={t('extraction.symbolNormalizeRegexHelper')}>
            <FieldInput
              aria-label={t('extraction.symbolNormalizeRegex')}
              className="w-56 font-mono text-[13px]"
              value={value.searchTitleExtraction.symbolNormalizeRegex}
              onChange={(e) => setExtraction({ symbolNormalizeRegex: e.target.value })}
            />
          </FormRow>
          <div className="py-3">
            <p className="text-sm text-ink-2">{t('extraction.charMappings')}</p>
            <p className="mt-0.5 text-xs text-ink-3">{t('extraction.charMappingsHelper')}</p>
            <div className="mt-2 flex flex-col gap-1.5">
              {value.searchTitleExtraction.charMappings.map((m, i) => (
                <div key={i} className="flex items-center gap-1.5">
                  <FieldInput
                    aria-label={t('extraction.mappingSource', { index: i + 1 })}
                    className="w-24 font-mono text-[13px]"
                    value={m.from}
                    onChange={(e) => setCharMapping(i, { from: e.target.value })}
                  />
                  <span className="text-sm text-ink-3">→</span>
                  <FieldInput
                    aria-label={t('extraction.mappingReplacement', { index: i + 1 })}
                    className="w-24 font-mono text-[13px]"
                    value={m.to}
                    onChange={(e) => setCharMapping(i, { to: e.target.value })}
                  />
                  <IconButton
                    label={t('extraction.removeMapping')}
                    className="size-8"
                    onClick={() =>
                      setExtraction({ charMappings: value.searchTitleExtraction.charMappings.filter((_, j) => j !== i) })
                    }
                  >
                    <X className="size-4" />
                  </IconButton>
                </div>
              ))}
              <div>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() =>
                    setExtraction({
                      charMappings: [...value.searchTitleExtraction.charMappings, { from: '', to: '' }],
                    })
                  }
                >
                  <Plus className="size-4" /> {t('extraction.addMapping')}
                </Button>
              </div>
            </div>
          </div>
          <div className="py-3">
            <p className="text-sm text-ink-2">{t('extraction.cleanupRegexes')}</p>
            <p className="mt-0.5 text-xs text-ink-3">{t('extraction.cleanupRegexesHelper')}</p>
            <div className="mt-2">
              <StringListInput
                aria-label={t('extraction.cleanupRegexes')}
                value={value.searchTitleExtraction.cleanupRegex}
                onChange={(v) => setExtraction({ cleanupRegex: v })}
              />
            </div>
          </div>
        </>
      )}
    </>
  )
}

export function ChineseConversionFields({ value, onChange }: ProcessingGroupProps) {
  const { t } = useTranslation('admin-komf')
  const set = (patch: Partial<ProcessingDraft>) => onChange({ ...value, ...patch })
  const setConversion = (patch: Partial<ProcessingDraft['chineseConversion']>) =>
    set({ chineseConversion: { ...value.chineseConversion, ...patch } })

  return (
    <>
      <FormRow label={t('enabled')} helper={t('chinese.enabledHelper')}>
        <Switch
          checked={value.chineseConversion.enabled}
          onCheckedChange={(v) => setConversion({ enabled: v })}
          label={t('section.chineseConversion')}
        />
      </FormRow>
      {value.chineseConversion.enabled && (
        <>
          <FormRow label={t('chinese.direction')}>
            <SegmentedControl
              options={CHINESE_DIRECTION_OPTIONS.map((o) => ({ value: o.value, label: t(o.labelKey) }))}
              value={value.chineseConversion.direction}
              onChange={(v) => setConversion({ direction: v })}
            />
          </FormRow>
          <FormRow label={t('chinese.convertSearch')}>
            <Switch
              checked={value.chineseConversion.search}
              onCheckedChange={(v) => setConversion({ search: v })}
              label={t('chinese.convertSearch')}
            />
          </FormRow>
          <FormRow label={t('chinese.convertMatching')}>
            <Switch
              checked={value.chineseConversion.matching}
              onCheckedChange={(v) => setConversion({ matching: v })}
              label={t('chinese.convertMatching')}
            />
          </FormRow>
          <FormRow label={t('chinese.convertUpdate')}>
            <Switch
              checked={value.chineseConversion.updateEnabled}
              onCheckedChange={(v) => setConversion({ updateEnabled: v })}
              label={t('chinese.convertUpdate')}
            />
          </FormRow>
          {value.chineseConversion.updateEnabled && (
            <div className="py-3">
              <p className="text-sm text-ink-2">{t('chinese.convertedFields')}</p>
              <div className="mt-2">
                <Checklist
                  options={CHINESE_FIELD_OPTIONS.map((o) => ({ value: o.value, label: t(o.labelKey) }))}
                  values={value.chineseConversion.updateFields}
                  onChange={(v) => setConversion({ updateFields: v })}
                  className="sm:grid-cols-4"
                />
              </div>
            </div>
          )}
        </>
      )}
    </>
  )
}

export function MylarFields({ value, onChange }: ProcessingGroupProps) {
  const { t } = useTranslation('admin-komf')
  const set = (patch: Partial<ProcessingDraft>) => onChange({ ...value, ...patch })

  return (
    <>
      <FormRow label={t('mylar.downloadCovers')} helper={t('mylar.downloadCoversHelper')}>
        <Switch
          checked={value.mylarCovers}
          onCheckedChange={(v) => set({ mylarCovers: v })}
          label={t('mylar.downloadCovers')}
        />
      </FormRow>
      <FormRow label={t('mylar.outputDir')} helper={t('mylar.outputDirHelper')}>
        <FieldInput
          aria-label={t('mylar.outputDir')}
          className="w-56"
          value={value.mylarOutputDir}
          onChange={(e) => set({ mylarOutputDir: e.target.value })}
        />
      </FormRow>
    </>
  )
}

/** Full processing editor with group dividers, for override rows where all groups share one card. */
export function ProcessingFields({ value, onChange }: ProcessingGroupProps) {
  const { t } = useTranslation('admin-komf')
  return (
    <div>
      <ProcessingGeneralFields value={value} onChange={onChange} />
      <Subheading>{t('section.postProcessing')}</Subheading>
      <PostProcessingFields value={value} onChange={onChange} />
      <Subheading>{t('section.searchTitleExtraction')}</Subheading>
      <SearchTitleExtractionFields value={value} onChange={onChange} />
      <Subheading>{t('section.chineseConversion')}</Subheading>
      <ChineseConversionFields value={value} onChange={onChange} />
      <Subheading>{t('section.mylar')}</Subheading>
      <MylarFields value={value} onChange={onChange} />
    </div>
  )
}
