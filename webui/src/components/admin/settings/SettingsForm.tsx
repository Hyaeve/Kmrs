import { useMemo, useState, type ReactNode } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { AnimatePresence, motion } from 'motion/react'
import { settingsApi } from '@/lib/api/settings'
import type { SettingsDto, SettingsUpdateDto, ThumbnailSize } from '@/lib/api/types'
import { useChanged } from '@/lib/hooks/useChanged'
import { Button } from '@/components/ui/Button'
import { SegmentedControl } from '@/components/ui/SegmentedControl'
import { Switch } from '@/components/ui/Switch'
import { Section } from '@/components/account/Section'
import { computeChanges, draftFromSettings, validateDraft, type SettingsDraft } from './draft'
import { FieldInput } from './FieldInput'
import { MultiSourceField } from './MultiSourceField'
import { RegenerateThumbnailsDialog } from './RegenerateThumbnailsDialog'
import { RotateKeyButton } from './RotateKeyButton'

const THUMBNAIL_OPTIONS: Array<{ value: ThumbnailSize; labelKey: string }> = [
  { value: 'DEFAULT', labelKey: 'thumbnail.default' },
  { value: 'MEDIUM', labelKey: 'thumbnail.medium' },
  { value: 'LARGE', labelKey: 'thumbnail.large' },
  { value: 'XLARGE', labelKey: 'thumbnail.xlarge' },
]

function FormRow({ label, helper, children }: { label: string; helper?: string; children: ReactNode }) {
  return (
    <div className="flex flex-wrap items-center justify-between gap-x-6 gap-y-2.5 py-3 first:pt-0 last:pb-0">
      <div className="min-w-0 max-w-md">
        <p className="text-sm text-ink-2">{label}</p>
        {helper && <p className="mt-0.5 text-xs text-ink-3">{helper}</p>}
      </div>
      {children}
    </div>
  )
}

export function SettingsForm({ settings }: { settings: SettingsDto }) {
  const { t } = useTranslation('admin-settings')
  const queryClient = useQueryClient()
  const [draft, setDraft] = useState<SettingsDraft>(() => draftFromSettings(settings))
  const [regenerateOpen, setRegenerateOpen] = useState(false)

  // structural sharing keeps the reference stable unless values actually changed (e.g. after save)
  if (useChanged([settings])) setDraft(draftFromSettings(settings))

  const changes = useMemo(() => computeChanges(settings, draft), [settings, draft])
  const errors = useMemo(() => validateDraft(draft), [draft])
  const dirtyCount = Object.keys(changes).length
  const hasErrors = Object.keys(errors).length > 0

  const save = useMutation({
    mutationFn: (body: SettingsUpdateDto) => settingsApi.update(body),
    onSuccess: (_data, body) => {
      queryClient.invalidateQueries({ queryKey: ['admin', 'settings'] })
      // existing books keep their old thumbnails, so a size change is a no-op without regeneration
      if (body.thumbnailSize) setRegenerateOpen(true)
    },
  })

  const set = <K extends keyof SettingsDraft>(key: K, value: SettingsDraft[K]) =>
    setDraft((d) => ({ ...d, [key]: value }))

  const discard = () => {
    setDraft(draftFromSettings(settings))
    save.reset()
  }

  const thumbnailOptions = THUMBNAIL_OPTIONS.map((o) => ({ value: o.value, label: t(o.labelKey) }))

  return (
    <div className="space-y-6">
      <Section title={t('sections.content')}>
        <FormRow label={t('form.deleteEmptyCollections')}>
          <Switch
            checked={draft.deleteEmptyCollections}
            onCheckedChange={(v) => set('deleteEmptyCollections', v)}
            label={t('form.deleteEmptyCollections')}
          />
        </FormRow>
        <FormRow label={t('form.deleteEmptyReadLists')}>
          <Switch
            checked={draft.deleteEmptyReadLists}
            onCheckedChange={(v) => set('deleteEmptyReadLists', v)}
            label={t('form.deleteEmptyReadLists')}
          />
        </FormRow>
      </Section>

      <Section title={t('sections.security')}>
        <FormRow label={t('form.rememberMeDuration')} helper={t('form.rememberMeDurationHelper')}>
          <div className="flex flex-col items-end gap-1">
            <div className="flex items-center gap-2">
              <FieldInput
                aria-label={t('form.rememberMeDurationDays')}
                className="w-24 text-right"
                inputMode="numeric"
                value={draft.rememberMeDurationDays}
                onChange={(e) => set('rememberMeDurationDays', e.target.value)}
                invalid={!!errors.rememberMeDurationDays}
              />
              <span className="text-sm text-ink-3">{t('form.days')}</span>
            </div>
            {errors.rememberMeDurationDays && <p className="text-xs text-danger">{errors.rememberMeDurationDays}</p>}
          </div>
        </FormRow>
        <FormRow label={t('form.rememberMeKey')} helper={t('form.rememberMeKeyHelper')}>
          <RotateKeyButton />
        </FormRow>
      </Section>

      <Section title={t('sections.thumbnails')}>
        <FormRow label={t('form.thumbnailSize')} helper={t('form.thumbnailSizeHelper')}>
          <SegmentedControl options={thumbnailOptions} value={draft.thumbnailSize} onChange={(v) => set('thumbnailSize', v)} />
        </FormRow>
      </Section>

      <Section title={t('sections.tasks')}>
        <FormRow label={t('form.taskPoolSize')} helper={t('form.taskPoolSizeHelper')}>
          <div className="flex flex-col items-end gap-1">
            <FieldInput
              aria-label={t('form.taskPoolSize')}
              className="w-24 text-right"
              inputMode="numeric"
              value={draft.taskPoolSize}
              onChange={(e) => set('taskPoolSize', e.target.value)}
              invalid={!!errors.taskPoolSize}
            />
            {errors.taskPoolSize && <p className="text-xs text-danger">{errors.taskPoolSize}</p>}
          </div>
        </FormRow>
      </Section>

      <Section title={t('sections.server')}>
        <FormRow label={t('form.serverPort')}>
          <MultiSourceField
            ariaLabel={t('form.serverPort')}
            source={settings.serverPort}
            value={draft.serverPort}
            onChange={(v) => set('serverPort', v)}
            error={errors.serverPort}
            inputMode="numeric"
          />
        </FormRow>
        <FormRow label={t('form.contextPath')} helper={t('form.contextPathHelper')}>
          <MultiSourceField
            ariaLabel={t('form.contextPath')}
            source={settings.serverContextPath}
            value={draft.serverContextPath}
            onChange={(v) => set('serverContextPath', v)}
            error={errors.serverContextPath}
          />
        </FormRow>
      </Section>

      <Section title={t('sections.kobo')}>
        <FormRow label={t('form.koboProxy')}>
          <Switch checked={draft.koboProxy} onCheckedChange={(v) => set('koboProxy', v)} label={t('form.koboProxy')} />
        </FormRow>
        <FormRow label={t('form.koboPort')}>
          <div className="flex flex-col items-end gap-1">
            <FieldInput
              aria-label={t('form.koboPort')}
              className="w-24 text-right"
              inputMode="numeric"
              value={draft.koboPort}
              onChange={(e) => set('koboPort', e.target.value)}
              invalid={!!errors.koboPort}
            />
            {errors.koboPort && <p className="text-xs text-danger">{errors.koboPort}</p>}
          </div>
        </FormRow>
      </Section>

      <Section title={t('sections.kepubify')}>
        <FormRow label={t('form.kepubifyPath')} helper={t('form.kepubifyPathHelper')}>
          <MultiSourceField
            ariaLabel={t('form.kepubifyPath')}
            source={settings.kepubifyPath}
            value={draft.kepubifyPath}
            onChange={(v) => set('kepubifyPath', v)}
          />
        </FormRow>
      </Section>

      {dirtyCount > 0 && <div className="h-16" aria-hidden />}

      <RegenerateThumbnailsDialog open={regenerateOpen} onOpenChange={setRegenerateOpen} />

      <AnimatePresence>
        {dirtyCount > 0 && (
          <div className="pointer-events-none fixed inset-x-0 bottom-[max(1.5rem,env(safe-area-inset-bottom))] z-20 flex justify-center px-4">
            <motion.div
              initial={{ opacity: 0, y: 24 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: 24 }}
              transition={{ duration: 0.2, ease: [0.16, 1, 0.3, 1] }}
              className="pointer-events-auto flex w-full max-w-lg flex-col gap-1.5 rounded-xl border border-line bg-overlay px-4 py-3 shadow-pop"
            >
              <div className="flex items-center gap-3">
                <span className="flex-1 text-sm text-ink-2">{t('form.unsaved', { count: dirtyCount })}</span>
                <Button size="sm" variant="ghost" onClick={discard} disabled={save.isPending}>
                  {t('form.discard')}
                </Button>
                <Button
                  size="sm"
                  variant="primary"
                  loading={save.isPending}
                  disabled={hasErrors}
                  onClick={() => save.mutate(changes)}
                >
                  {t('common:action.save')}
                </Button>
              </div>
              {hasErrors && <p className="text-xs text-ink-3">{t('form.fixInvalid')}</p>}
              {save.isError && (
                <p className="text-xs text-danger">
                  {save.error instanceof Error ? save.error.message : t('form.saveFailed')}
                </p>
              )}
            </motion.div>
          </div>
        )}
      </AnimatePresence>
    </div>
  )
}
