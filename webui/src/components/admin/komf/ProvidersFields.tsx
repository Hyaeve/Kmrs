import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { CaretRight, Plus } from '@phosphor-icons/react'
import type { KomfAuthorRole, KomfLibraryType, KomfNameMatchingMode } from '@/lib/api/types'
import { cn } from '@/lib/utils/cn'
import type { BookMetadataDraft, KomfProviderKey, ProviderDraft, ProvidersDraft, SeriesMetadataDraft } from './draft'
import {
  AUTHOR_ROLE_OPTIONS,
  PROVIDERS_WITH_BOOKS,
  PROVIDERS_WITH_MEDIA_TYPE,
  PROVIDER_KEYS,
  PROVIDER_LABELS,
} from './draft'
import { FieldInput } from '@/components/admin/settings/FieldInput'
import { Button } from '@/components/ui/Button'
import { Menu, MenuItem } from '@/components/ui/Menu'
import { SegmentedControl } from '@/components/ui/SegmentedControl'
import { Switch } from '@/components/ui/Switch'
import { Checklist, CollapsibleGroup } from './fields'
import { FormRow } from './FormRow'
import { ProviderExtras } from './ProviderExtras'

const NAME_MATCHING_OPTIONS: Array<{ value: KomfNameMatchingMode | ''; labelKey: string }> = [
  { value: '', labelKey: 'nameMatching.inherit' },
  { value: 'CLOSEST_MATCH', labelKey: 'nameMatching.closestMatch' },
  { value: 'EXACT', labelKey: 'nameMatching.exact' },
]

const MEDIA_TYPE_OPTIONS: Array<{ value: KomfLibraryType; labelKey: string }> = [
  { value: 'MANGA', labelKey: 'libraryType.manga' },
  { value: 'NOVEL', labelKey: 'libraryType.novel' },
]

type BooleanKeys<T> = { [K in keyof T]: T[K] extends boolean ? K : never }[keyof T]

const SERIES_METADATA_OPTIONS: Array<{ value: BooleanKeys<SeriesMetadataDraft>; labelKey: string }> = [
  { value: 'status', labelKey: 'metadata:field.status' },
  { value: 'title', labelKey: 'metadata:field.title' },
  { value: 'alternativeTitles', labelKey: 'fields.alternativeTitles' },
  { value: 'summary', labelKey: 'metadata:field.summary' },
  { value: 'publisher', labelKey: 'metadata:field.publisher' },
  { value: 'readingDirection', labelKey: 'metadata:field.readingDirection' },
  { value: 'ageRating', labelKey: 'metadata:field.ageRating' },
  { value: 'language', labelKey: 'metadata:field.language' },
  { value: 'genres', labelKey: 'metadata:field.genres' },
  { value: 'tags', labelKey: 'metadata:field.tags' },
  { value: 'totalBookCount', labelKey: 'fields.bookCount' },
  { value: 'authors', labelKey: 'metadata:field.authors' },
  { value: 'releaseDate', labelKey: 'metadata:field.releaseDate' },
  { value: 'thumbnail', labelKey: 'fields.cover' },
  { value: 'links', labelKey: 'fields.links' },
  { value: 'score', labelKey: 'fields.score' },
  { value: 'useOriginalPublisher', labelKey: 'fields.useOriginalPublisher' },
]

const BOOK_METADATA_OPTIONS: Array<{ value: BooleanKeys<BookMetadataDraft>; labelKey: string }> = [
  { value: 'title', labelKey: 'metadata:field.title' },
  { value: 'summary', labelKey: 'metadata:field.summary' },
  { value: 'number', labelKey: 'metadata:field.number' },
  { value: 'numberSort', labelKey: 'fields.numberSort' },
  { value: 'releaseDate', labelKey: 'metadata:field.releaseDate' },
  { value: 'authors', labelKey: 'metadata:field.authors' },
  { value: 'tags', labelKey: 'metadata:field.tags' },
  { value: 'isbn', labelKey: 'metadata:field.isbn' },
  { value: 'links', labelKey: 'fields.links' },
  { value: 'thumbnail', labelKey: 'fields.cover' },
]

const ROLE_LABEL_KEYS: Record<KomfAuthorRole, string> = {
  WRITER: 'authorRole.writer',
  PENCILLER: 'authorRole.penciller',
  INKER: 'authorRole.inker',
  COLORIST: 'authorRole.colorist',
  LETTERER: 'authorRole.letterer',
  COVER: 'authorRole.cover',
  EDITOR: 'authorRole.editor',
  TRANSLATOR: 'authorRole.translator',
}

const ROLE_OPTIONS = AUTHOR_ROLE_OPTIONS.map((r) => ({ value: r, labelKey: ROLE_LABEL_KEYS[r] }))

interface ProviderCardProps {
  provider: KomfProviderKey
  value: ProviderDraft
  onChange: (v: ProviderDraft) => void
  error?: string
  /** expand the options on mount — used for a freshly added provider */
  defaultOpen?: boolean
}

function ProviderCard({ provider, value, onChange, error, defaultOpen }: ProviderCardProps) {
  const { t } = useTranslation('admin-komf')
  const [open, setOpen] = useState(defaultOpen ?? false)
  const label = PROVIDER_LABELS[provider]
  const set = (patch: Partial<ProviderDraft>) => onChange({ ...value, ...patch })
  const setSeries = (patch: Partial<SeriesMetadataDraft>) =>
    set({ seriesMetadata: { ...value.seriesMetadata, ...patch } })
  const setBook = (patch: Partial<BookMetadataDraft>) => set({ bookMetadata: { ...value.bookMetadata, ...patch } })
  const seriesFieldCount = SERIES_METADATA_OPTIONS.filter((o) => value.seriesMetadata[o.value]).length
  const bookFieldCount = BOOK_METADATA_OPTIONS.filter((o) => value.bookMetadata[o.value]).length
  const roleOptions = ROLE_OPTIONS.map((o) => ({ value: o.value, label: t(o.labelKey) }))

  return (
    <div className="rounded-lg border border-line">
      <div className="flex items-center gap-3 px-3 py-2.5">
        <button
          type="button"
          onClick={() => setOpen((v) => !v)}
          aria-label={t('providers.options', { name: label })}
          aria-expanded={open}
          className="flex min-w-0 flex-1 cursor-pointer items-center gap-2 text-left"
        >
          <CaretRight
            className={cn('size-3.5 shrink-0 text-ink-3 transition-transform duration-150', open && 'rotate-90')}
          />
          <span className="truncate text-sm font-medium text-ink">{label}</span>
        </button>
        <FieldInput
          aria-label={t('providers.priority', { name: label })}
          className="w-20 text-right"
          inputMode="numeric"
          value={value.priority}
          onChange={(e) => set({ priority: e.target.value })}
          invalid={!!error}
        />
        <Switch
          checked={value.enabled}
          onCheckedChange={(v) => set({ enabled: v })}
          label={t('providers.enabled', { name: label })}
        />
      </div>
      {open && (
        <div className="border-t border-line px-3 pb-3">
          {PROVIDERS_WITH_MEDIA_TYPE.has(provider) && (
            <FormRow label={t('providers.mediaType')}>
              <SegmentedControl
                options={MEDIA_TYPE_OPTIONS.map((o) => ({ value: o.value, label: t(o.labelKey) }))}
                value={value.mediaType}
                onChange={(v) => set({ mediaType: v })}
              />
            </FormRow>
          )}
          <FormRow label={t('nameMatching.label')}>
            <SegmentedControl
              options={NAME_MATCHING_OPTIONS.map((o) => ({ value: o.value, label: t(o.labelKey) }))}
              value={value.nameMatchingMode}
              onChange={(v) => set({ nameMatchingMode: v })}
            />
          </FormRow>
          <div className="py-3">
            <p className="text-sm text-ink-2">{t('providers.authorRoles')}</p>
            <div className="mt-2">
              <Checklist
                options={roleOptions}
                values={value.authorRoles}
                onChange={(v) => set({ authorRoles: v })}
              />
            </div>
          </div>
          <div className="py-3">
            <p className="text-sm text-ink-2">{t('providers.artistRoles')}</p>
            <div className="mt-2">
              <Checklist
                options={roleOptions}
                values={value.artistRoles}
                onChange={(v) => set({ artistRoles: v })}
              />
            </div>
          </div>

          <CollapsibleGroup
            label={t('providers.seriesMetadata')}
            summary={`${seriesFieldCount}/${SERIES_METADATA_OPTIONS.length}`}
          >
            <Checklist
              options={SERIES_METADATA_OPTIONS.map((o) => ({ value: o.value, label: t(o.labelKey) }))}
              values={SERIES_METADATA_OPTIONS.filter((o) => value.seriesMetadata[o.value]).map((o) => o.value)}
              onChange={(v) => {
                const patch: Partial<SeriesMetadataDraft> = {}
                for (const o of SERIES_METADATA_OPTIONS) patch[o.value] = v.includes(o.value)
                setSeries(patch)
              }}
              className="sm:grid-cols-3"
            />
            <FormRow label={t('processing.originalPublisherTagName')}>
              <FieldInput
                aria-label={t('processing.originalPublisherTagName')}
                className="w-36"
                value={value.seriesMetadata.originalPublisherTagName}
                onChange={(e) => setSeries({ originalPublisherTagName: e.target.value })}
              />
            </FormRow>
            <FormRow label={t('providers.englishPublisherTagName')}>
              <FieldInput
                aria-label={t('providers.englishPublisherTagName')}
                className="w-36"
                value={value.seriesMetadata.englishPublisherTagName}
                onChange={(e) => setSeries({ englishPublisherTagName: e.target.value })}
              />
            </FormRow>
            <FormRow label={t('providers.frenchPublisherTagName')}>
              <FieldInput
                aria-label={t('providers.frenchPublisherTagName')}
                className="w-36"
                value={value.seriesMetadata.frenchPublisherTagName}
                onChange={(e) => setSeries({ frenchPublisherTagName: e.target.value })}
              />
            </FormRow>
          </CollapsibleGroup>

          {PROVIDERS_WITH_BOOKS.has(provider) && (
            <CollapsibleGroup
              label={t('providers.bookMetadata')}
              summary={
                value.seriesMetadata.books
                  ? `${bookFieldCount}/${BOOK_METADATA_OPTIONS.length}`
                  : t('providers.bookMetadataOff')
              }
            >
              <FormRow label={t('providers.updateBookMetadata')}>
                <Switch
                  checked={value.seriesMetadata.books}
                  onCheckedChange={(v) => setSeries({ books: v })}
                  label={t('providers.updateBookMetadata')}
                />
              </FormRow>
              {value.seriesMetadata.books && (
                <div className="pb-3">
                  <Checklist
                    options={BOOK_METADATA_OPTIONS.map((o) => ({ value: o.value, label: t(o.labelKey) }))}
                    values={BOOK_METADATA_OPTIONS.filter((o) => value.bookMetadata[o.value]).map((o) => o.value)}
                    onChange={(v) => {
                      const patch: Partial<BookMetadataDraft> = {}
                      for (const o of BOOK_METADATA_OPTIONS) patch[o.value] = v.includes(o.value)
                      setBook(patch)
                    }}
                    className="sm:grid-cols-3"
                  />
                </div>
              )}
            </CollapsibleGroup>
          )}

          <ProviderExtras provider={provider} value={value} onChange={onChange} />
        </div>
      )}
      {error && <p className="px-3 pb-2 text-xs text-danger">{t(error)}</p>}
    </div>
  )
}

interface ProvidersFieldsProps {
  value: ProvidersDraft
  onChange: (v: ProvidersDraft) => void
  errors?: Partial<Record<KomfProviderKey, string>>
}

/** One expandable card per enabled provider; disabled providers are added back from the menu. */
export function ProvidersFields({ value, onChange, errors }: ProvidersFieldsProps) {
  const { t } = useTranslation('admin-komf')
  const [addedKey, setAddedKey] = useState<KomfProviderKey | null>(null)
  const enabled = PROVIDER_KEYS.filter((key) => value[key].enabled)
  const available = PROVIDER_KEYS.filter((key) => !value[key].enabled)

  return (
    <div className="flex flex-col gap-2">
      {enabled.length === 0 && <p className="text-sm text-ink-3">{t('providers.empty')}</p>}
      {enabled.map((key) => (
        <ProviderCard
          key={key}
          provider={key}
          value={value[key]}
          onChange={(v) => onChange({ ...value, [key]: v })}
          error={errors?.[key]}
          defaultOpen={key === addedKey}
        />
      ))}
      {available.length > 0 && (
        <Menu
          align="start"
          trigger={
            <Button size="sm" variant="secondary" className="self-start">
              <Plus className="size-4" /> {t('providers.add')}
            </Button>
          }
        >
          {available.map((key) => (
            <MenuItem
              key={key}
              onSelect={() => {
                setAddedKey(key)
                onChange({ ...value, [key]: { ...value[key], enabled: true } })
              }}
            >
              {PROVIDER_LABELS[key]}
            </MenuItem>
          ))}
        </Menu>
      )}
    </div>
  )
}
