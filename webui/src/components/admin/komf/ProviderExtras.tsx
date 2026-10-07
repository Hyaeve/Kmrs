import { useTranslation } from 'react-i18next'
import type { KomfProviderKey, ProviderDraft } from './draft'
import { MANGADEX_LINK_OPTIONS } from './draft'
import { FieldInput } from '@/components/admin/settings/FieldInput'
import { SegmentedControl } from '@/components/ui/SegmentedControl'
import { Switch } from '@/components/ui/Switch'
import { Checklist, SelectInput, StringListInput, Subheading } from './fields'
import { FormRow } from './FormRow'

interface ProviderExtrasProps {
  provider: KomfProviderKey
  value: ProviderDraft
  onChange: (v: ProviderDraft) => void
}

function ArchiveFields({ provider, value, onChange }: ProviderExtrasProps) {
  const { t } = useTranslation('admin-komf')
  const set = (patch: Partial<ProviderDraft>) => onChange({ ...value, ...patch })
  return (
    <>
      <Subheading>{t('extras.archive.title')}</Subheading>
      <FormRow label={t('enabled')} helper={t('extras.archive.enabledHelper')}>
        <Switch
          checked={value.archiveEnabled}
          onCheckedChange={(v) => set({ archiveEnabled: v })}
          label={t('extras.archive.title')}
        />
      </FormRow>
      {provider === 'bangumi' && (
        <FormRow label={t('extras.archive.directory')}>
          <FieldInput
            aria-label={t('extras.archive.directory')}
            className="w-56"
            value={value.archiveDir}
            onChange={(e) => set({ archiveDir: e.target.value })}
          />
        </FormRow>
      )}
      {provider === 'eHentai' && (
        <>
          <FormRow label={t('extras.archive.downloadUrl')} helper={t('extras.archive.downloadUrlHelper')}>
            <FieldInput
              aria-label={t('extras.archive.downloadUrl')}
              className="w-56"
              value={value.archiveUrl}
              onChange={(e) => set({ archiveUrl: e.target.value })}
            />
          </FormRow>
          <FormRow label={t('extras.archive.dbFile')}>
            <FieldInput
              aria-label={t('extras.archive.dbFile')}
              className="w-56"
              value={value.archiveDbFile}
              onChange={(e) => set({ archiveDbFile: e.target.value })}
            />
          </FormRow>
        </>
      )}
      <FormRow label={t('extras.archive.updateInterval')}>
        <FieldInput
          aria-label={t('extras.archive.updateInterval')}
          className="w-24 text-right"
          inputMode="numeric"
          value={value.archiveUpdateIntervalHours}
          onChange={(e) => set({ archiveUpdateIntervalHours: e.target.value })}
        />
      </FormRow>
      <FormRow label={t('extras.archive.idleRelease')} helper={t('extras.archive.idleReleaseHelper')}>
        <FieldInput
          aria-label={t('extras.archive.idleRelease')}
          className="w-24 text-right"
          inputMode="numeric"
          value={value.archiveIdleReleaseSecs}
          onChange={(e) => set({ archiveIdleReleaseSecs: e.target.value })}
        />
      </FormRow>
      {provider === 'eHentai' && (
        <>
          <div className="py-3">
            <p className="text-sm text-ink-2">{t('extras.archive.categoryFilter')}</p>
            <p className="mt-0.5 text-xs text-ink-3">{t('extras.archive.categoryFilterHelper')}</p>
            <div className="mt-2">
              <StringListInput
                aria-label={t('extras.archive.categoryFilter')}
                value={value.archiveSearchCategoryFilter}
                onChange={(v) => set({ archiveSearchCategoryFilter: v })}
              />
            </div>
          </div>
          <div className="py-3">
            <p className="text-sm text-ink-2">{t('extras.archive.uploaderFilter')}</p>
            <p className="mt-0.5 text-xs text-ink-3">{t('extras.archive.uploaderFilterHelper')}</p>
            <div className="mt-2">
              <StringListInput
                aria-label={t('extras.archive.uploaderFilter')}
                value={value.archiveSearchUploaderFilter}
                onChange={(v) => set({ archiveSearchUploaderFilter: v })}
              />
            </div>
          </div>
        </>
      )}
    </>
  )
}

/** Provider-specific settings beyond the shared enable/priority/field-toggle blocks. */
export function ProviderExtras({ provider, value, onChange }: ProviderExtrasProps) {
  const { t } = useTranslation('admin-komf')
  const set = (patch: Partial<ProviderDraft>) => onChange({ ...value, ...patch })

  switch (provider) {
    case 'aniList':
      return (
        <>
          <FormRow label={t('extras.tagsScoreThreshold')}>
            <FieldInput
              aria-label={t('extras.tagsScoreThreshold')}
              className="w-24 text-right"
              inputMode="numeric"
              value={value.tagsScoreThreshold}
              onChange={(e) => set({ tagsScoreThreshold: e.target.value })}
            />
          </FormRow>
          <FormRow label={t('extras.tagsSizeLimit')}>
            <FieldInput
              aria-label={t('extras.tagsSizeLimit')}
              className="w-24 text-right"
              inputMode="numeric"
              value={value.tagsSizeLimit}
              onChange={(e) => set({ tagsSizeLimit: e.target.value })}
            />
          </FormRow>
        </>
      )
    case 'mangaDex':
      return (
        <>
          <div className="py-3">
            <p className="text-sm text-ink-2">{t('extras.coverLanguages')}</p>
            <p className="mt-0.5 text-xs text-ink-3">{t('processing.bcp47Helper')}</p>
            <div className="mt-2">
              <StringListInput
                aria-label={t('extras.coverLanguages')}
                value={value.coverLanguages}
                onChange={(v) => set({ coverLanguages: v })}
              />
            </div>
          </div>
          <div className="py-3">
            <p className="text-sm text-ink-2">{t('fields.links')}</p>
            <p className="mt-0.5 text-xs text-ink-3">{t('extras.mangaDexLinksHelper')}</p>
            <div className="mt-2">
              <Checklist
                options={MANGADEX_LINK_OPTIONS.map((l) => ({ value: l, label: l.replaceAll('_', ' ') }))}
                values={value.links}
                onChange={(v) => set({ links: v })}
                className="sm:grid-cols-3"
              />
            </div>
          </div>
        </>
      )
    case 'mangaBaka':
      return (
        <>
          <FormRow label={t('extras.mode')} helper={t('extras.modeHelper')}>
            <SegmentedControl
              options={[
                { value: 'API', label: 'API' },
                { value: 'DATABASE', label: t('extras.modeDatabase') },
              ]}
              value={value.mode}
              onChange={(v) => set({ mode: v })}
            />
          </FormRow>
          <div className="py-3">
            <p className="text-sm text-ink-2">{t('extras.coverLanguages')}</p>
            <p className="mt-0.5 text-xs text-ink-3">{t('processing.bcp47Helper')}</p>
            <div className="mt-2">
              <StringListInput
                aria-label={t('extras.coverLanguages')}
                value={value.coverLanguages}
                onChange={(v) => set({ coverLanguages: v })}
              />
            </div>
          </div>
          <FormRow label={t('extras.updateInterval')} helper={t('extras.updateIntervalHelper')}>
            <FieldInput
              aria-label={t('extras.updateInterval')}
              className="w-24 text-right"
              inputMode="numeric"
              value={value.updateIntervalHours}
              onChange={(e) => set({ updateIntervalHours: e.target.value })}
            />
          </FormRow>
        </>
      )
    case 'bookWalker':
      return (
        <FormRow label={t('extras.updateInterval')} helper={t('extras.updateIntervalHelper')}>
          <FieldInput
            aria-label={t('extras.updateInterval')}
            className="w-24 text-right"
            inputMode="numeric"
            value={value.updateIntervalHours}
            onChange={(e) => set({ updateIntervalHours: e.target.value })}
          />
        </FormRow>
      )
    case 'bangumi':
      return (
        <>
          <div className="py-3">
            <p className="text-sm text-ink-2">{t('extras.tagWhitelist')}</p>
            <p className="mt-0.5 text-xs text-ink-3">{t('extras.tagWhitelistHelper')}</p>
            <div className="mt-2">
              <StringListInput
                aria-label={t('extras.tagWhitelist')}
                value={value.tagWhitelist}
                onChange={(v) => set({ tagWhitelist: v })}
              />
            </div>
          </div>
          <FormRow label={t('extras.tagWhitelistFile')} helper={t('extras.tagWhitelistFileHelper')}>
            <FieldInput
              aria-label={t('extras.tagWhitelistFile')}
              className="w-56"
              value={value.tagWhitelistFile}
              onChange={(e) => set({ tagWhitelistFile: e.target.value })}
            />
          </FormRow>
          <ArchiveFields provider={provider} value={value} onChange={onChange} />
        </>
      )
    case 'eHentai':
      return (
        <>
          <div className="py-3">
            <p className="text-sm text-ink-2">{t('extras.preferredLanguages')}</p>
            <p className="mt-0.5 text-xs text-ink-3">{t('processing.bcp47Helper')}</p>
            <div className="mt-2">
              <StringListInput
                aria-label={t('extras.preferredLanguages')}
                value={value.preferredLanguages}
                onChange={(v) => set({ preferredLanguages: v })}
              />
            </div>
          </div>
          <FormRow label={t('extras.titlePriority')}>
            <SelectInput
              aria-label={t('extras.titlePriority')}
              options={[
                { value: 'jpn', label: t('extras.titlePriorityValue.jpn') },
                { value: 'eng', label: t('extras.titlePriorityValue.eng') },
              ]}
              value={value.titlePriority}
              onChange={(v) => set({ titlePriority: v })}
            />
          </FormRow>
          <div className="py-3">
            <p className="text-sm text-ink-2">{t('extras.translatorKeywords')}</p>
            <p className="mt-0.5 text-xs text-ink-3">{t('extras.translatorKeywordsHelper')}</p>
            <div className="mt-2">
              <StringListInput
                aria-label={t('extras.translatorKeywords')}
                value={value.translatorKeywords}
                onChange={(v) => set({ translatorKeywords: v })}
              />
            </div>
          </div>
          <FormRow label={t('extras.maleOnlyTagsFile')}>
            <FieldInput
              aria-label={t('extras.maleOnlyTagsFile')}
              className="w-56"
              value={value.maleOnlyTagsFile}
              onChange={(e) => set({ maleOnlyTagsFile: e.target.value })}
            />
          </FormRow>
          <FormRow label={t('extras.titleTemplate')} helper={t('extras.titleTemplateHelper')}>
            <FieldInput
              aria-label={t('extras.titleTemplate')}
              className="w-56"
              value={value.titleTemplate}
              onChange={(e) => set({ titleTemplate: e.target.value })}
            />
          </FormRow>
          <FormRow label={t('extras.translateTags')}>
            <Switch
              checked={value.tagTranslationEnabled}
              onCheckedChange={(v) => set({ tagTranslationEnabled: v })}
              label={t('extras.translateTags')}
            />
          </FormRow>
          <FormRow label={t('extras.tagTranslationUrl')} helper={t('extras.tagTranslationUrlHelper')}>
            <FieldInput
              aria-label={t('extras.tagTranslationUrl')}
              className="w-56"
              value={value.tagTranslationUrl}
              onChange={(e) => set({ tagTranslationUrl: e.target.value })}
            />
          </FormRow>
          <FormRow label={t('extras.gidOnlyMatch')} helper={t('extras.gidOnlyMatchHelper')}>
            <Switch
              checked={value.gidOnlyMatch}
              onCheckedChange={(v) => set({ gidOnlyMatch: v })}
              label={t('extras.gidOnlyMatch')}
            />
          </FormRow>
          <FormRow label={t('extras.searchDomain')}>
            <SelectInput
              aria-label={t('extras.searchDomain')}
              options={[
                { value: 'e-hentai', label: 'e-hentai' },
                { value: 'exhentai', label: 'exhentai' },
              ]}
              value={value.searchDomain}
              onChange={(v) => set({ searchDomain: v })}
            />
          </FormRow>
          <FormRow label={t('extras.ipbMemberId')} helper={t('extras.ipbCookieHelper')}>
            <FieldInput
              aria-label={t('extras.ipbMemberId')}
              className="w-56"
              value={value.ipbMemberId}
              onChange={(e) => set({ ipbMemberId: e.target.value })}
            />
          </FormRow>
          <FormRow label={t('extras.ipbPassHash')} helper={t('extras.ipbCookieHelper')}>
            <FieldInput
              aria-label={t('extras.ipbPassHash')}
              className="w-56"
              value={value.ipbPassHash}
              onChange={(e) => set({ ipbPassHash: e.target.value })}
            />
          </FormRow>
          <ArchiveFields provider={provider} value={value} onChange={onChange} />
        </>
      )
    default:
      return null
  }
}
