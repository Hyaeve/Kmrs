import { useMemo, useState } from 'react'
import { useSearchParams } from 'react-router-dom'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { AnimatePresence, motion } from 'motion/react'
import { komfApi } from '@/lib/api/komf'
import type { KomfConfig, KomfConfigPatch, KomfNameMatchingMode, LibraryDto } from '@/lib/api/types'
import { useChanged } from '@/lib/hooks/useChanged'
import { Section } from '@/components/account/Section'
import { Button } from '@/components/ui/Button'
import { SegmentedControl } from '@/components/ui/SegmentedControl'
import {
  countChanges,
  computeChanges,
  draftFromConfig,
  hasErrors,
  validateDraft,
  type KomfConfigDraft,
  type ProcessingDraft,
  type ProvidersDraft,
} from './draft'
import { EventListenerFields } from './EventListenerFields'
import { LibraryOverrides } from './LibraryOverrides'
import { NotificationsFields } from './NotificationsFields'
import { OAuthAccountsFields } from './OAuthAccountsFields'
import {
  ChineseConversionFields,
  MylarFields,
  PostProcessingFields,
  ProcessingGeneralFields,
  SearchTitleExtractionFields,
} from './ProcessingFields'
import { ProviderCredentialsFields } from './ProviderCredentialsFields'
import { ProvidersFields } from './ProvidersFields'
import { TabBar } from './TabBar'
import { FormRow } from './FormRow'

const NAME_MATCHING_OPTIONS: Array<{ value: KomfNameMatchingMode; labelKey: string }> = [
  { value: 'CLOSEST_MATCH', labelKey: 'nameMatching.closestMatch' },
  { value: 'EXACT', labelKey: 'nameMatching.exact' },
]

const KOMF_TAB_IDS = ['providers', 'metadata', 'listener', 'notifications', 'overrides'] as const
type KomfTabId = (typeof KOMF_TAB_IDS)[number]

const KOMF_TABS: ReadonlyArray<{ id: KomfTabId; labelKey: string }> = [
  { id: 'providers', labelKey: 'tabs.providers' },
  { id: 'metadata', labelKey: 'tabs.metadata' },
  { id: 'listener', labelKey: 'tabs.listener' },
  { id: 'notifications', labelKey: 'tabs.notifications' },
  { id: 'overrides', labelKey: 'tabs.overrides' },
]

function tabFromParam(param: string | null): KomfTabId {
  return (KOMF_TAB_IDS as readonly string[]).includes(param ?? '') ? (param as KomfTabId) : 'providers'
}

export function KomfConfigForm({ config, libraries }: { config: KomfConfig; libraries: LibraryDto[] }) {
  const { t, i18n } = useTranslation('admin-komf')
  const queryClient = useQueryClient()
  const [draft, setDraft] = useState<KomfConfigDraft>(() => draftFromConfig(config))

  // structural sharing keeps the reference stable unless values actually changed (e.g. after save)
  if (useChanged([config])) setDraft(draftFromConfig(config))

  const changes = useMemo(() => computeChanges(config, draft), [config, draft])
  const errors = useMemo(() => validateDraft(draft), [draft])
  const dirtyCount = countChanges(changes)
  const invalid = hasErrors(errors)

  const save = useMutation({
    mutationFn: (body: KomfConfigPatch) => komfApi.patchConfig(body),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['admin', 'komf-config'] }),
  })

  const discard = () => {
    setDraft(draftFromConfig(config))
    save.reset()
  }

  const sortedLibraries = useMemo(
    () => libraries.slice().sort((a, b) => a.name.localeCompare(b.name, i18n.language)),
    [libraries, i18n.language],
  )

  const patch = (p: Partial<KomfConfigDraft>) => setDraft((d) => ({ ...d, ...p }))

  const toggleNotifyLibrary = (id: string, on: boolean) =>
    patch({
      notificationsLibraryFilter: on
        ? [...draft.notificationsLibraryFilter, id]
        : draft.notificationsLibraryFilter.filter((x) => x !== id),
    })

  const setLibraryProcessing = (id: string, v: ProcessingDraft | undefined) =>
    setDraft((d) => {
      const next = { ...d.libraryProcessing }
      if (v) next[id] = v
      else delete next[id]
      return { ...d, libraryProcessing: next }
    })

  const setLibraryProviders = (id: string, v: ProvidersDraft | undefined) =>
    setDraft((d) => {
      const next = { ...d.libraryProviders }
      if (v) next[id] = v
      else delete next[id]
      return { ...d, libraryProviders: next }
    })

  const [searchParams, setSearchParams] = useSearchParams()
  const tab = tabFromParam(searchParams.get('tab'))

  const setTab = (id: KomfTabId) => {
    if (id === tab) return
    const next = new URLSearchParams(searchParams)
    next.set('tab', id)
    setSearchParams(next)
  }

  const tabErrors: Record<KomfTabId, boolean> = {
    providers: errors.comicVineSearchLimit !== undefined || Object.keys(errors.defaultProviders).length > 0,
    metadata: errors.defaultProcessingMessages.length > 0,
    listener: false,
    notifications: false,
    overrides: Object.keys(errors.libraryProviders).length > 0 || errors.libraryProcessingMessages.length > 0,
  }

  const setProcessing = (v: ProcessingDraft) => patch({ defaultProcessing: v })

  return (
    <div className="space-y-6">
      <div className="sticky top-[calc(var(--spacing)*15+env(safe-area-inset-top))] z-10 bg-bg/80 backdrop-blur-md">
        <TabBar
          tabs={KOMF_TABS.map((tab) => ({ id: tab.id, label: t(tab.labelKey), hasError: tabErrors[tab.id] }))}
          active={tab}
          onChange={setTab}
        />
      </div>

      {tab === 'providers' && (
        <>
          <Section title={t('tabs.providers')}>
            <ProvidersFields
              value={draft.defaultProviders}
              onChange={(v) => patch({ defaultProviders: v })}
              errors={errors.defaultProviders}
            />
            <FormRow label={t('nameMatching.label')}>
              <SegmentedControl
                options={NAME_MATCHING_OPTIONS.map((o) => ({ value: o.value, label: t(o.labelKey) }))}
                value={draft.nameMatchingMode}
                onChange={(v) => patch({ nameMatchingMode: v })}
              />
            </FormRow>
          </Section>
          <Section title={t('section.providerCredentials')}>
            <ProviderCredentialsFields value={draft} onChange={patch} searchLimitError={errors.comicVineSearchLimit} />
          </Section>
          <Section title={t('oauth.title')}>
            <OAuthAccountsFields />
          </Section>
        </>
      )}

      {tab === 'metadata' && (
        <>
          <Section title={t('metadata:tabs.general')}>
            <div>
              <ProcessingGeneralFields value={draft.defaultProcessing} onChange={setProcessing} />
            </div>
          </Section>
          <Section title={t('section.postProcessing')}>
            <div>
              <PostProcessingFields value={draft.defaultProcessing} onChange={setProcessing} />
            </div>
          </Section>
          <Section title={t('section.searchTitleExtraction')}>
            <div>
              <SearchTitleExtractionFields value={draft.defaultProcessing} onChange={setProcessing} />
            </div>
          </Section>
          <Section title={t('section.chineseConversion')}>
            <div>
              <ChineseConversionFields value={draft.defaultProcessing} onChange={setProcessing} />
            </div>
          </Section>
          <Section title={t('section.mylar')}>
            <div>
              <MylarFields value={draft.defaultProcessing} onChange={setProcessing} />
            </div>
          </Section>
        </>
      )}

      {tab === 'listener' && (
        <Section title={t('section.eventListener')}>
          <EventListenerFields value={draft} libraries={sortedLibraries} onChange={patch} />
        </Section>
      )}

      {tab === 'notifications' && (
        <Section title={t('tabs.notifications')}>
          <NotificationsFields value={draft.notifications} onChange={(v) => patch({ notifications: v })} />
          <div className="pt-3">
            <p className="text-sm text-ink-2">{t('notifications.notifyLibraries')}</p>
            <p className="mt-0.5 text-xs text-ink-3">{t('notifications.notifyLibrariesHelper')}</p>
            <div className="mt-2 grid gap-1.5 sm:grid-cols-2">
              {sortedLibraries.map((lib) => (
                <label key={lib.id} className="flex cursor-pointer items-center gap-2 text-sm text-ink-2">
                  <input
                    type="checkbox"
                    checked={draft.notificationsLibraryFilter.includes(lib.id)}
                    onChange={(e) => toggleNotifyLibrary(lib.id, e.target.checked)}
                    className="size-4 shrink-0 cursor-pointer accent-accent"
                  />
                  <span className="truncate">{lib.name}</span>
                </label>
              ))}
            </div>
          </div>
        </Section>
      )}

      {tab === 'overrides' && (
        <Section title={t('section.libraryOverrides')}>
          <LibraryOverrides
            libraries={sortedLibraries}
            config={config}
            processing={draft.libraryProcessing}
            providers={draft.libraryProviders}
            providerErrors={errors.libraryProviders}
            onProcessingChange={setLibraryProcessing}
            onProvidersChange={setLibraryProviders}
          />
        </Section>
      )}

      {dirtyCount > 0 && <div className="h-16" aria-hidden />}

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
                <span className="flex-1 text-sm text-ink-2">{t('saveBar.unsavedChanges', { count: dirtyCount })}</span>
                <Button size="sm" variant="ghost" onClick={discard} disabled={save.isPending}>
                  {t('saveBar.discard')}
                </Button>
                <Button
                  size="sm"
                  variant="primary"
                  loading={save.isPending}
                  disabled={invalid}
                  onClick={() => save.mutate(changes)}
                >
                  {t('common:action.save')}
                </Button>
              </div>
              {invalid && <p className="text-xs text-ink-3">{t('saveBar.invalidHint')}</p>}
              {errors.messages.map((m) => (
                <p key={m} className="text-xs text-danger">
                  {t(m)}
                </p>
              ))}
              {save.isError && (
                <p className="text-xs text-danger">
                  {save.error instanceof Error ? save.error.message : t('saveBar.saveFailed')}
                </p>
              )}
            </motion.div>
          </div>
        )}
      </AnimatePresence>
    </div>
  )
}
