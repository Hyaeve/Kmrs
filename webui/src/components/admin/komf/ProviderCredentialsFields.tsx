import { useTranslation } from 'react-i18next'
import type { KomfConfigDraft } from './draft'
import { TextField } from '@/components/ui/TextField'
import { SelectInput } from './fields'

const COMICVINE_ID_FORMAT_OPTIONS: Array<{ value: string; labelKey: string }> = [
  { value: '', labelKey: 'noOverride' },
  { value: 'SERIES', labelKey: 'credentials.idFormatValue.series' },
  { value: 'VOLUME', labelKey: 'credentials.idFormatValue.volume' },
  { value: 'ISSUE', labelKey: 'credentials.idFormatValue.issue' },
]

interface ProviderCredentialsFieldsProps {
  value: KomfConfigDraft
  onChange: (patch: Partial<KomfConfigDraft>) => void
  searchLimitError?: string
}

export function ProviderCredentialsFields({ value, onChange, searchLimitError }: ProviderCredentialsFieldsProps) {
  const { t } = useTranslation('admin-komf')
  return (
    <div className="flex flex-col gap-4">
      <TextField
        label={t('credentials.malClientId')}
        helper={t('credentials.emptyClears')}
        value={value.malClientId}
        onChange={(e) => onChange({ malClientId: e.target.value })}
      />
      <TextField
        label={t('credentials.comicVineApiKey')}
        helper={t('credentials.emptyClears')}
        value={value.comicVineApiKey}
        onChange={(e) => onChange({ comicVineApiKey: e.target.value })}
      />
      <TextField
        label={t('credentials.comicVineSearchLimit')}
        helper={t('credentials.comicVineSearchLimitHelper')}
        inputMode="numeric"
        value={value.comicVineSearchLimit}
        onChange={(e) => onChange({ comicVineSearchLimit: e.target.value })}
        error={searchLimitError ? t(searchLimitError) : undefined}
      />
      <TextField
        label={t('credentials.comicVineIssueName')}
        helper={t('credentials.emptyClears')}
        value={value.comicVineIssueName}
        onChange={(e) => onChange({ comicVineIssueName: e.target.value })}
      />
      <div className="flex flex-col gap-2">
        <p className="text-[13px] font-medium text-ink-2">{t('credentials.comicVineIdFormat')}</p>
        <SelectInput
          aria-label={t('credentials.comicVineIdFormat')}
          className="h-10 w-full"
          options={COMICVINE_ID_FORMAT_OPTIONS.map((o) => ({ value: o.value, label: t(o.labelKey) }))}
          value={value.comicVineIdFormat}
          onChange={(v) => onChange({ comicVineIdFormat: v })}
        />
      </div>
      <TextField
        label={t('credentials.bangumiToken')}
        helper={t('credentials.emptyClears')}
        value={value.bangumiToken}
        onChange={(e) => onChange({ bangumiToken: e.target.value })}
      />
    </div>
  )
}
