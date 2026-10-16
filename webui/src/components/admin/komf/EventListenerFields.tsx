import { useTranslation } from 'react-i18next'
import type { LibraryDto } from '@/lib/api/types'
import type { KomfConfigDraft } from './draft'
import { Switch } from '@/components/ui/Switch'
import { StringListInput } from './fields'
import { FormRow } from './FormRow'

interface EventListenerFieldsProps {
  value: KomfConfigDraft
  libraries: LibraryDto[]
  onChange: (patch: Partial<KomfConfigDraft>) => void
}

export function EventListenerFields({ value, libraries, onChange }: EventListenerFieldsProps) {
  const { t } = useTranslation('admin-komf')
  const toggleLibrary = (id: string, on: boolean) =>
    onChange({
      eventListenerLibraryFilter: on
        ? [...value.eventListenerLibraryFilter, id]
        : value.eventListenerLibraryFilter.filter((x) => x !== id),
    })

  return (
    <div>
      <FormRow label={t('enabled')}>
        <Switch
          checked={value.eventListenerEnabled}
          onCheckedChange={(v) => onChange({ eventListenerEnabled: v })}
          label={t('section.eventListener')}
        />
      </FormRow>
      <div className="pt-3">
        <p className="text-sm text-ink-2">{t('listener.libraries')}</p>
        <p className="mt-0.5 text-xs text-ink-3">{t('listener.librariesHelper')}</p>
        <div className="mt-2 grid gap-1.5 sm:grid-cols-2">
          {libraries.map((lib) => (
            <label key={lib.id} className="flex cursor-pointer items-center gap-2 text-sm text-ink-2">
              <input
                type="checkbox"
                checked={value.eventListenerLibraryFilter.includes(lib.id)}
                onChange={(e) => toggleLibrary(lib.id, e.target.checked)}
                className="size-4 shrink-0 cursor-pointer accent-accent"
              />
              <span className="truncate">{lib.name}</span>
            </label>
          ))}
        </div>
      </div>
      <div className="pt-3">
        <p className="text-sm text-ink-2">{t('listener.excludedSeries')}</p>
        <p className="mt-0.5 text-xs text-ink-3">{t('listener.excludedSeriesHelper')}</p>
        <div className="mt-2">
          <StringListInput
            aria-label={t('listener.excludedSeries')}
            value={value.eventListenerSeriesExcludeFilter}
            onChange={(v) => onChange({ eventListenerSeriesExcludeFilter: v })}
          />
        </div>
      </div>
    </div>
  )
}
