import { useId, useMemo, useState, type FormEvent, type ReactNode } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { CaretDown, CaretUp, WarningCircle, X } from '@phosphor-icons/react'
import { librariesApi } from '@/lib/api/libraries'
import type { LibraryDto, ScanInterval, SeriesCover } from '@/lib/api/types'
import { useChanged } from '@/lib/hooks/useChanged'
import { cn } from '@/lib/utils/cn'
import { Button } from '@/components/ui/Button'
import { Dialog } from '@/components/ui/Dialog'
import { SegmentedControl } from '@/components/ui/SegmentedControl'
import { Switch } from '@/components/ui/Switch'
import { TextField } from '@/components/ui/TextField'
import { FilesystemDialog } from './FilesystemDialog'
import {
  ANALYSIS_FIELDS,
  BATCH_ENABLE_KEYS,
  DEFAULTS,
  diffCreation,
  diffUpdate,
  formFromLibrary,
  IMPORT_FIELDS,
  parseViolations,
  RESCAN_PATCH_KEYS,
  SCAN_INTERVAL_OPTIONS,
  SCAN_SWITCH_FIELDS,
  SERIES_COVER_OPTIONS,
  type LibraryFormState,
} from './model'

interface LibraryDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** present = edit mode, absent = create mode */
  library?: LibraryDto
}

export function LibraryDialog({ open, onOpenChange, library }: LibraryDialogProps) {
  const { t } = useTranslation('admin-maintenance')
  const queryClient = useQueryClient()
  const isEdit = !!library
  // edit mode mounts fresh with open=true, so useChanged never fires on mount — seed from props here
  const [form, setForm] = useState<LibraryFormState>(() =>
    library ? formFromLibrary(library) : { ...DEFAULTS, scanDirectoryExclusions: [] },
  )
  const [advancedOpen, setAdvancedOpen] = useState(false)
  const [browsingRoot, setBrowsingRoot] = useState(false)

  if (useChanged([open, library]) && open) {
    setForm(library ? formFromLibrary(library) : { ...DEFAULTS, scanDirectoryExclusions: [] })
    setAdvancedOpen(false)
    setBrowsingRoot(false)
  }

  const set = <K extends keyof LibraryFormState>(key: K, value: LibraryFormState[K]) =>
    setForm((f) => ({ ...f, [key]: value }))

  const save = useMutation({
    mutationFn: async () => {
      if (library) await librariesApi.update(library.id, diffUpdate(form, library))
      else await librariesApi.create(diffCreation(form))
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['libraries'] })
      onOpenChange(false)
    },
  })

  // the exact patch that would be sent; warnings key off it so they only show for real changes
  const patch = useMemo(() => (library ? diffUpdate(form, library) : null), [form, library])
  const hasChanges = !!patch && Object.keys(patch).length > 0
  const triggersRescan = !!patch && RESCAN_PATCH_KEYS.some((k) => k in patch)
  const triggersBatch = !!patch && BATCH_ENABLE_KEYS.some((k) => patch[k] === true)

  const violations = save.error ? parseViolations(save.error) : []
  const violationFor = (field: string) => violations.find((v) => v.fieldName === field)?.message
  const otherViolations = violations.filter((v) => !['name', 'root', 'oneshotsDirectory'].includes(v.fieldName))
  const generalError =
    save.error && violations.length === 0
      ? save.error instanceof Error
        ? save.error.message
        : t('errorFallback')
      : null

  const canSubmit = !!form.name.trim() && !!form.root.trim() && (!isEdit || hasChanges)
  const onSubmit = (e: FormEvent) => {
    e.preventDefault()
    if (canSubmit) save.mutate()
  }

  return (
    <>
      <Dialog open={open} onOpenChange={onOpenChange} title={isEdit ? t('libraries.editTitle') : t('libraries.add')} size="lg">
        <form onSubmit={onSubmit} className="flex h-full min-h-0 flex-col">
          <div className="min-h-0 flex-1 space-y-7 overflow-y-auto px-5 py-4">
            <FormSection title={t('libraries.general')} hint={isEdit ? undefined : t('libraries.generalHint')}>
              <div className="flex flex-col gap-4">
                <TextField
                  label={t('metadata:field.name')}
                  value={form.name}
                  onChange={(e) => set('name', e.target.value)}
                  error={violationFor('name')}
                  placeholder={t('libraries.namePlaceholder')}
                  autoFocus
                  required
                />
                <PathField
                  label={t('libraries.rootFolder')}
                  value={form.root}
                  onChange={(v) => set('root', v)}
                  onBrowse={() => setBrowsingRoot(true)}
                  error={violationFor('root')}
                  helper={t('libraries.rootHelper')}
                />
                <TextField
                  label={t('libraries.oneshotsDirectory')}
                  value={form.oneshotsDirectory}
                  onChange={(e) => set('oneshotsDirectory', e.target.value)}
                  error={violationFor('oneshotsDirectory')}
                  helper={t('libraries.oneshotsHelper')}
                  placeholder={t('libraries.oneshotsPlaceholder')}
                  spellCheck={false}
                />
                {!isEdit && <p className="text-[13px] text-ink-3">{t('libraries.autoScanNote')}</p>}
              </div>
            </FormSection>

            {!isEdit && (
              <button
                type="button"
                onClick={() => setAdvancedOpen((v) => !v)}
                className="flex cursor-pointer items-center gap-1.5 text-[13px] font-medium text-ink-3 transition-colors hover:text-ink"
              >
                {advancedOpen ? <CaretUp className="size-3.5" /> : <CaretDown className="size-3.5" />}
                {t('libraries.advanced')}
              </button>
            )}

            {(isEdit || advancedOpen) && (
              <>
                <FormSection title={t('libraries.scanning')}>
                  <div className="flex flex-col gap-1">
                    <div className="flex flex-wrap items-center justify-between gap-x-4 gap-y-2 py-1.5">
                      <span className="text-sm text-ink-2">{t('libraries.scanIntervalLabel')}</span>
                      <SegmentedControl<ScanInterval>
                        size="sm"
                        options={SCAN_INTERVAL_OPTIONS.map((o) => ({ value: o.value, label: t(o.labelKey) }))}
                        value={form.scanInterval}
                        onChange={(v) => set('scanInterval', v)}
                      />
                    </div>
                    <div className="grid gap-x-8 sm:grid-cols-2">
                      {SCAN_SWITCH_FIELDS.map(({ key, labelKey }) => (
                        <SwitchRow key={key} label={t(labelKey)} checked={form[key]} onChange={(v) => set(key, v)} />
                      ))}
                    </div>
                    <ExclusionsEditor value={form.scanDirectoryExclusions} onChange={(v) => set('scanDirectoryExclusions', v)} />
                  </div>
                </FormSection>

                <FormSection title={t('libraries.importSources')} hint={t('libraries.importSourcesHint')}>
                  <div className="grid gap-x-8 sm:grid-cols-2">
                    {IMPORT_FIELDS.map(({ key, labelKey }) => (
                      <SwitchRow key={key} label={t(labelKey)} checked={form[key]} onChange={(v) => set(key, v)} />
                    ))}
                  </div>
                </FormSection>

                <FormSection title={t('libraries.analysisMaintenance')}>
                  <div className="grid gap-x-8 sm:grid-cols-2">
                    {ANALYSIS_FIELDS.map(({ key, labelKey }) => (
                      <SwitchRow key={key} label={t(labelKey)} checked={form[key]} onChange={(v) => set(key, v)} />
                    ))}
                  </div>
                </FormSection>

                <FormSection title={t('libraries.seriesCover')}>
                  <SegmentedControl<SeriesCover>
                    size="sm"
                    options={SERIES_COVER_OPTIONS.map((o) => ({ value: o.value, label: t(o.labelKey) }))}
                    value={form.seriesCover}
                    onChange={(v) => set('seriesCover', v)}
                  />
                </FormSection>
              </>
            )}
          </div>

          <div className="shrink-0 space-y-3 border-t border-line px-5 py-3.5">
            {(triggersRescan || triggersBatch) && (
              <div className="flex items-start gap-2 rounded-lg border border-accent/25 bg-accent-soft px-3 py-2 text-[13px] text-accent-strong">
                <WarningCircle className="mt-0.5 size-4 shrink-0" />
                <div className="space-y-0.5">
                  {triggersRescan && <p>{t('libraries.rescanWarning')}</p>}
                  {triggersBatch && <p>{t('libraries.batchWarning')}</p>}
                </div>
              </div>
            )}
            {otherViolations.length > 0 && (
              <ul className="space-y-0.5 text-[13px] text-danger">
                {otherViolations.map((v, i) => (
                  <li key={i}>
                    {v.fieldName}: {v.message}
                  </li>
                ))}
              </ul>
            )}
            {generalError && <p className="text-[13px] text-danger">{generalError}</p>}
            <div className="flex justify-end gap-2">
              <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
                {t('common:action.cancel')}
              </Button>
              <Button type="submit" variant="primary" loading={save.isPending} disabled={!canSubmit}>
                {isEdit ? t('metadata:action.saveChanges') : t('libraries.add')}
              </Button>
            </div>
          </div>
        </form>
      </Dialog>

      <FilesystemDialog
        open={browsingRoot}
        onOpenChange={setBrowsingRoot}
        initialPath={form.root.trim() || undefined}
        onSelect={(p) => {
          set('root', p)
          setBrowsingRoot(false)
        }}
      />
    </>
  )
}

function FormSection({ title, hint, children }: { title: string; hint?: string; children: ReactNode }) {
  return (
    <section>
      <h3 className="text-[13px] font-semibold tracking-wide text-ink uppercase">{title}</h3>
      {hint && <p className="mt-0.5 text-[13px] text-ink-3">{hint}</p>}
      <div className="mt-3">{children}</div>
    </section>
  )
}

function PathField({
  label,
  value,
  onChange,
  onBrowse,
  helper,
  error,
}: {
  label: string
  value: string
  onChange: (v: string) => void
  onBrowse: () => void
  helper?: string
  error?: string
}) {
  const { t } = useTranslation('admin-maintenance')
  const id = useId()
  return (
    <div className="flex flex-col gap-2">
      <label htmlFor={id} className="text-[13px] font-medium text-ink-2">
        {label}
      </label>
      <div className="flex gap-2">
        <input
          id={id}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          spellCheck={false}
          placeholder="/path/to/folder"
          aria-invalid={!!error}
          className={cn(
            'h-10 min-w-0 flex-1 rounded-lg border bg-surface px-3 font-mono text-base text-ink transition-colors',
            'placeholder:text-ink-3 focus:outline-none',
            error ? 'border-danger/60 focus:border-danger' : 'border-line focus:border-accent/70',
          )}
        />
        <Button type="button" onClick={onBrowse}>
          {t('libraries.browse')}
        </Button>
      </div>
      {error ? <p className="text-[13px] text-danger">{error}</p> : helper ? <p className="text-[13px] text-ink-3">{helper}</p> : null}
    </div>
  )
}

function SwitchRow({ label, checked, onChange }: { label: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <div className="flex items-center justify-between gap-4 py-1.5">
      <span className="text-sm text-ink-2">{label}</span>
      <Switch checked={checked} onCheckedChange={onChange} label={label} />
    </div>
  )
}

function ExclusionsEditor({ value, onChange }: { value: string[]; onChange: (v: string[]) => void }) {
  const { t } = useTranslation('admin-maintenance')
  const [text, setText] = useState('')
  const add = () => {
    const v = text.trim()
    if (!v || value.includes(v)) return
    onChange([...value, v])
    setText('')
  }
  return (
    <div className="flex flex-col gap-2 pt-2">
      <span className="text-[13px] font-medium text-ink-2">{t('libraries.exclusions')}</span>
      <div className="flex gap-2">
        <input
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault()
              add()
            }
          }}
          spellCheck={false}
          placeholder={t('libraries.exclusionPlaceholder')}
          className="h-10 min-w-0 flex-1 rounded-lg border border-line bg-surface px-3 font-mono text-base text-ink transition-colors placeholder:text-ink-3 focus:border-accent/70 focus:outline-none"
        />
        <Button type="button" onClick={add} disabled={!text.trim()}>
          {t('common:action.add')}
        </Button>
      </div>
      {value.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {value.map((ex) => (
            <span
              key={ex}
              className="inline-flex items-center gap-1 rounded-full border border-line bg-raised py-1 pr-1.5 pl-3 font-mono text-xs text-ink-2"
            >
              {ex}
              <button
                type="button"
                onClick={() => onChange(value.filter((v) => v !== ex))}
                aria-label={t('libraries.removeExclusion', { name: ex })}
                className="cursor-pointer rounded-full p-0.5 text-ink-3 transition-colors hover:text-danger"
              >
                <X className="size-3" />
              </button>
            </span>
          ))}
        </div>
      )}
      <p className="text-[13px] text-ink-3">{t('libraries.exclusionsHelper')}</p>
    </div>
  )
}
