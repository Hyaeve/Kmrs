import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { CaretRight, Plus } from '@phosphor-icons/react'
import type { KomfConfig, LibraryDto } from '@/lib/api/types'
import { Button } from '@/components/ui/Button'
import { Chip } from '@/components/ui/Chip'
import { Dialog } from '@/components/ui/Dialog'
import { Menu, MenuItem } from '@/components/ui/Menu'
import { Switch } from '@/components/ui/Switch'
import {
  processingDraftFromConfig,
  providersDraftFromMap,
  type KomfProviderKey,
  type ProcessingDraft,
  type ProvidersDraft,
} from './draft'
import { FormRow } from './FormRow'
import { ProcessingFields } from './ProcessingFields'
import { ProvidersFields } from './ProvidersFields'

interface LibraryOverrideDialogProps {
  library: LibraryDto
  open: boolean
  onOpenChange: (open: boolean) => void
  processing: ProcessingDraft | undefined
  processingSeed: ProcessingDraft
  providers: ProvidersDraft | undefined
  providersSeed: ProvidersDraft
  providerErrors: Partial<Record<KomfProviderKey, string>> | undefined
  onProcessingChange: (v: ProcessingDraft | undefined) => void
  onProvidersChange: (v: ProvidersDraft | undefined) => void
}

function LibraryOverrideDialog({
  library,
  open,
  onOpenChange,
  processing,
  processingSeed,
  providers,
  providersSeed,
  providerErrors,
  onProcessingChange,
  onProvidersChange,
}: LibraryOverrideDialogProps) {
  const { t } = useTranslation('admin-komf')
  return (
    <Dialog open={open} onOpenChange={onOpenChange} title={library.name} size="lg">
      <div className="px-5 py-4">
        <FormRow label={t('overrides.overrideProcessing')} helper={t('overrides.overrideProcessingHelper')}>
          <Switch
            checked={!!processing}
            onCheckedChange={(on) => onProcessingChange(on ? processingSeed : undefined)}
            label={t('overrides.overrideProcessingFor', { name: library.name })}
          />
        </FormRow>
        {processing && (
          <div className="mb-2 border-t border-line pt-4">
            <ProcessingFields value={processing} onChange={onProcessingChange} />
          </div>
        )}
        <FormRow label={t('overrides.overrideProviders')} helper={t('overrides.overrideProvidersHelper')}>
          <Switch
            checked={!!providers}
            onCheckedChange={(on) => onProvidersChange(on ? providersSeed : undefined)}
            label={t('overrides.overrideProvidersFor', { name: library.name })}
          />
        </FormRow>
        {providers && (
          <div className="border-t border-line pt-4">
            <ProvidersFields value={providers} onChange={onProvidersChange} errors={providerErrors} />
          </div>
        )}
      </div>
      <div className="flex items-center justify-between gap-3 border-t border-line px-5 py-3.5">
        <p className="text-xs text-ink-3">{t('overrides.dialogFooter')}</p>
        <Button variant="primary" className="shrink-0" onClick={() => onOpenChange(false)}>
          {t('common:action.done')}
        </Button>
      </div>
    </Dialog>
  )
}

interface LibraryOverridesProps {
  libraries: LibraryDto[]
  config: KomfConfig
  processing: Record<string, ProcessingDraft>
  providers: Record<string, ProvidersDraft>
  providerErrors: Record<string, Partial<Record<KomfProviderKey, string>>>
  onProcessingChange: (libId: string, v: ProcessingDraft | undefined) => void
  onProvidersChange: (libId: string, v: ProvidersDraft | undefined) => void
}

/** One row per library that has an override; other libraries are added from the menu, opening the same dialog. */
export function LibraryOverrides({
  libraries,
  config,
  processing,
  providers,
  providerErrors,
  onProcessingChange,
  onProvidersChange,
}: LibraryOverridesProps) {
  const { t } = useTranslation('admin-komf')
  const [dialogLibId, setDialogLibId] = useState<string | null>(null)
  const [dialogOpen, setDialogOpen] = useState(false)
  const overridden = libraries.filter((lib) => processing[lib.id] !== undefined || providers[lib.id] !== undefined)
  const available = libraries.filter((lib) => processing[lib.id] === undefined && providers[lib.id] === undefined)
  // kept after close so the dialog's exit animation can play
  const dialogLib = libraries.find((lib) => lib.id === dialogLibId)

  const openDialog = (libId: string) => {
    setDialogLibId(libId)
    setDialogOpen(true)
  }

  return (
    <div className="flex flex-col gap-2">
      {overridden.length === 0 && <p className="text-sm text-ink-3">{t('overrides.empty')}</p>}
      {overridden.map((lib) => {
        const libProcessing = processing[lib.id]
        const libProviders = providers[lib.id]
        const errors = providerErrors[lib.id]
        const hasProviderErrors = errors !== undefined && Object.keys(errors).length > 0
        return (
          <button
            key={lib.id}
            type="button"
            aria-haspopup="dialog"
            onClick={() => openDialog(lib.id)}
            className="flex w-full cursor-pointer items-center gap-2 rounded-lg border border-line px-3 py-2.5 text-left transition-colors duration-150 hover:bg-raised"
          >
            <span className="min-w-0 flex-1 truncate text-sm font-medium text-ink">{lib.name}</span>
            {libProcessing && <Chip className="shrink-0 px-2 py-0.5">{t('tabs.metadata')}</Chip>}
            {libProviders && <Chip className="shrink-0 px-2 py-0.5">{t('tabs.providers')}</Chip>}
            {hasProviderErrors && <span className="size-1.5 shrink-0 rounded-full bg-danger" aria-hidden />}
            <CaretRight className="size-3.5 shrink-0 text-ink-3" />
          </button>
        )
      })}
      {available.length > 0 && (
        <Menu
          align="start"
          trigger={
            <Button size="sm" variant="secondary" className="self-start">
              <Plus className="size-4" /> {t('overrides.add')}
            </Button>
          }
        >
          {available.map((lib) => (
            <MenuItem key={lib.id} onSelect={() => openDialog(lib.id)}>
              {lib.name}
            </MenuItem>
          ))}
        </Menu>
      )}
      {dialogLib && (
        <LibraryOverrideDialog
          library={dialogLib}
          open={dialogOpen}
          onOpenChange={setDialogOpen}
          processing={processing[dialogLib.id]}
          // komf seeds a new metadata-update override from the current global default
          processingSeed={processing[dialogLib.id] ?? processingDraftFromConfig(config.komga.metadataUpdate.default)}
          providers={providers[dialogLib.id]}
          // komf seeds a new provider override from its own built-ins, so send the
          // global defaults instead to end up with "global defaults + admin's edit"
          providersSeed={providers[dialogLib.id] ?? providersDraftFromMap(config.metadataProviders.defaultProviders)}
          providerErrors={providerErrors[dialogLib.id]}
          onProcessingChange={(v) => onProcessingChange(dialogLib.id, v)}
          onProvidersChange={(v) => onProvidersChange(dialogLib.id, v)}
        />
      )}
    </div>
  )
}
