import { useEffect, useMemo, useState } from 'react'
import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { CaretLeft, CaretRight, CircleNotch, WarningCircle } from '@phosphor-icons/react'
import { booksApi } from '@/lib/api/books'
import { collectionsApi, readlistsApi } from '@/lib/api/collections'
import { librariesApi } from '@/lib/api/libraries'
import type { ReferentialScope } from '@/lib/api/referential'
import { seriesApi } from '@/lib/api/series'
import { smartListsApi } from '@/lib/api/smartLists'
import type { BookDto, Page, SeriesDto, SmartListDto, SmartListTarget, SmartListVisibility } from '@/lib/api/types'
import { isAdmin, useAuthStore } from '@/lib/store/auth'
import { useChanged } from '@/lib/hooks/useChanged'
import { Button } from '@/components/ui/Button'
import { Dialog } from '@/components/ui/Dialog'
import { IconButton } from '@/components/ui/IconButton'
import { TextField } from '@/components/ui/TextField'
import { SegmentedControl } from '@/components/ui/SegmentedControl'
import { activeFilterCount } from '@/components/filters/filterUrl'
import { buildBookSearch, buildSeriesSearch } from '@/components/filters/builders'
import { EnumFilterSection, FilterGroupDetail, FilterGroupRow, FlagFilterRow } from '@/components/filters/FilterGroups'
import {
  SMARTLIST_BOOK_FILTER_GROUPS,
  SMARTLIST_SERIES_FILTER_GROUPS,
  type FilterGroupDef,
  type FilterState,
  type GroupKey,
} from '@/components/filters/types'
import { emptyFilterState, searchToFilterState, useFilterState } from './searchState'

interface EntityName {
  id: string
  name: string
}

function fetchEntityNames(kind: 'libraries' | 'collections' | 'readlists'): Promise<EntityName[]> {
  if (kind === 'libraries') return librariesApi.list()
  if (kind === 'collections') return collectionsApi.list({ unpaged: true }).then((p) => p.content)
  return readlistsApi.list({ unpaged: true }).then((p) => p.content)
}

function useEntityNames(kind: 'libraries' | 'collections' | 'readlists') {
  return useQuery({
    queryKey: ['entity-options', kind],
    queryFn: () => fetchEntityNames(kind),
    staleTime: 60_000,
  })
}

/** create or edit a smart list as a two-step wizard (details, then filters inline); `existing` absent means create */
export function SmartListDialog({
  open,
  onOpenChange,
  existing,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  existing?: SmartListDto
}) {
  const { t } = useTranslation('smartlists')
  const queryClient = useQueryClient()
  const admin = isAdmin(useAuthStore((s) => s.user))

  const [step, setStep] = useState<1 | 2>(1)
  const [openGroup, setOpenGroup] = useState<GroupKey | null>(null)
  const [name, setName] = useState(existing?.name ?? '')
  const [summary, setSummary] = useState(existing?.summary ?? '')
  const [target, setTarget] = useState<SmartListTarget>(existing?.target ?? 'BOOK')
  const [visibility, setVisibility] = useState<SmartListVisibility>(existing?.visibility ?? 'PRIVATE')
  const [sharedWith, setSharedWith] = useState<string[]>(existing?.sharedWithUserIds ?? [])
  const [submitError, setSubmitError] = useState<string | null>(null)

  const parsed = useMemo(
    () => (existing ? searchToFilterState(existing.search, existing.target) : { state: emptyFilterState(), lossy: false }),
    [existing],
  )
  const filters = useFilterState(parsed.state)

  if (useChanged([open, existing?.id, existing?.lastModifiedDate]) && open) {
    setStep(1)
    setOpenGroup(null)
    setName(existing?.name ?? '')
    setSummary(existing?.summary ?? '')
    setTarget(existing?.target ?? 'BOOK')
    setVisibility(existing?.visibility ?? 'PRIVATE')
    setSharedWith(existing?.sharedWithUserIds ?? [])
    setSubmitError(null)
    filters.reset(parsed.state)
  }

  // everyone can pick share targets; the directory is small and self-hosted servers are few-user
  const shareTargets = useQuery({
    queryKey: ['smart-lists', 'share-targets'],
    queryFn: () => smartListsApi.shareTargets(),
    // the directory is admin-only on the server; others always create private lists
    enabled: admin,
    staleTime: 60_000,
  })
  const currentUserId = useAuthStore((s) => s.user)?.id
  const shareOptions = (shareTargets.data ?? []).filter((u) => u.id !== currentUserId)

  const libraries = useEntityNames('libraries')
  const readlists = useEntityNames('readlists')
  const collections = useEntityNames('collections')
  const entityNames = useMemo(() => {
    const map = new Map<string, string>()
    for (const e of libraries.data ?? []) map.set(e.id, e.name)
    for (const e of readlists.data ?? []) map.set(e.id, e.name)
    for (const e of collections.data ?? []) map.set(e.id, e.name)
    return map
  }, [libraries.data, readlists.data, collections.data])
  const labelFor = (_key: GroupKey, id: string) => entityNames.get(id) ?? id

  const groups = useMemo(
    () =>
      (target === 'BOOK' ? SMARTLIST_BOOK_FILTER_GROUPS : SMARTLIST_SERIES_FILTER_GROUPS).filter(
        (g) => !g.adminOnly || admin,
      ),
    [target, admin],
  )
  const count = activeFilterCount(filters.state)
  const openDef = openGroup ? groups.find((g) => g.key === openGroup) : undefined

  // referential options follow the selected scope: one chosen membership list wins,
  // otherwise the union of the chosen libraries
  const scope: ReferentialScope | undefined = useMemo(() => {
    const membership = target === 'BOOK' ? filters.state.readlists : filters.state.collections
    if (membership.length === 1) {
      return target === 'BOOK' ? { readListId: membership[0] } : { collectionId: membership[0] }
    }
    if (filters.state.libraries.length > 0) return { libraryId: filters.state.libraries }
    return undefined
  }, [filters.state, target])

  // live match count for the edited filter: /list with size 1 reports the full total
  const previewSearch = useMemo(
    () => (target === 'BOOK' ? buildBookSearch(filters.state) : buildSeriesSearch(filters.state)),
    [filters.state, target],
  )
  // debounced so the match count does not fire a request per keystroke
  const [debouncedSearch, setDebouncedSearch] = useState(previewSearch)
  useEffect(() => {
    const t = setTimeout(() => setDebouncedSearch(previewSearch), 300)
    return () => clearTimeout(t)
  }, [previewSearch])
  const preview = useQuery({
    queryKey: ['smart-lists', 'preview', target, JSON.stringify(debouncedSearch)],
    queryFn: (): Promise<Page<BookDto | SeriesDto>> =>
      target === 'BOOK'
        ? booksApi.list({ search: debouncedSearch, page: 0, size: 1 })
        : seriesApi.list({ search: debouncedSearch, page: 0, size: 1 }),
    enabled: open && step === 2,
    placeholderData: keepPreviousData,
    staleTime: 5_000,
  })

  const mutation = useMutation({
    mutationFn: () => {
      const search = target === 'BOOK' ? buildBookSearch(filters.state) : buildSeriesSearch(filters.state)
      // visibility is an admin-only concern; others always create private lists
      const body = {
        name: name.trim(),
        summary: summary.trim(),
        target,
        ...(admin
          ? {
              visibility,
              ...(visibility === 'SHARED' ? { sharedWithUserIds: sharedWith } : {}),
            }
          : {}),
        search,
      }
      return existing
        ? smartListsApi.update(existing.id, body).then(() => undefined)
        : smartListsApi.create(body).then(() => undefined)
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['smart-lists'] })
      onOpenChange(false)
    },
    onError: (e: Error) => setSubmitError(e.message),
  })

  const changeTarget = (next: SmartListTarget) => {
    setTarget(next)
    // membership leaves only exist on their own target's side
    filters.reset({ ...filters.state, readlists: [], collections: [] } as FilterState)
  }

  const submit = (e: React.FormEvent) => {
    e.preventDefault()
    if (!name.trim()) return
    if (step === 1) {
      setOpenGroup(null)
      setStep(2)
    } else {
      mutation.mutate()
    }
  }

  // the same partition FilterDrawer uses, inlined so step 2 needs no overlay
  const enumGroups = groups.filter((g) => g.kind === 'enum' && !g.flag)
  const flagGroups = groups.filter((g) => g.flag)
  const letterGroups = groups.filter((g) => g.kind === 'letters')
  const navGroups = groups.filter((g) => g.kind !== 'enum' && g.kind !== 'letters' && !g.flag)

  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title={step === 1 ? (existing ? t('editTitle') : t('createTitle')) : t('filtersStepTitle')}
      size="lg"
      fill
    >
      <form onSubmit={submit} className="flex min-h-0 flex-1 flex-col">
        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
          {step === 1 ? (
            <div className="space-y-4">
              <TextField label={t('nameLabel')} value={name} onChange={(e) => setName(e.target.value)} autoFocus required />
              <div className="flex flex-col gap-2">
                <label htmlFor="smartlist-summary" className="text-[13px] font-medium text-ink-2">
                  {t('summaryLabel')}
                </label>
                <textarea
                  id="smartlist-summary"
                  rows={2}
                  value={summary}
                  onChange={(e) => setSummary(e.target.value)}
                  className="w-full rounded-lg border border-line bg-surface px-3 py-2 text-base text-ink transition-colors placeholder:text-ink-3 focus:border-accent/70 focus:outline-none"
                />
              </div>
              {!existing && (
                <div className="flex flex-col gap-2">
                  <span className="text-[13px] font-medium text-ink-2">{t('targetLabel')}</span>
                  <SegmentedControl
                    options={[
                      { value: 'BOOK', label: t('target.book') },
                      { value: 'SERIES', label: t('target.series') },
                    ]}
                    value={target}
                    onChange={(v) => changeTarget(v as SmartListTarget)}
                  />
                </div>
              )}
              {admin && (
                <div className="flex flex-col gap-2">
                  <span className="text-[13px] font-medium text-ink-2">{t('visibilityLabel')}</span>
                  <SegmentedControl
                    options={[
                      { value: 'PRIVATE', label: t('visibility.private') },
                      { value: 'PUBLIC', label: t('visibility.public') },
                      { value: 'SHARED', label: t('visibility.shared') },
                    ]}
                    value={visibility}
                    onChange={(v) => setVisibility(v as SmartListVisibility)}
                  />
                  {visibility === 'SHARED' && (
                    <div className="flex flex-col gap-2">
                      <span className="text-[13px] font-medium text-ink-2">{t('sharedWithLabel')}</span>
                      {shareTargets.isPending ? (
                        <p className="text-xs text-ink-3">{t('loadingUsers')}</p>
                      ) : shareOptions.length === 0 ? (
                        <p className="text-xs text-ink-3">{t('noOtherUsers')}</p>
                      ) : (
                        <div className="flex max-h-36 flex-col gap-1 overflow-y-auto rounded-lg border border-line bg-raised p-2">
                          {shareOptions.map((u) => (
                            <label key={u.id} className="flex cursor-pointer items-center gap-2 px-1 py-1 text-sm text-ink">
                              <input
                                type="checkbox"
                                checked={sharedWith.includes(u.id)}
                                onChange={() =>
                                  setSharedWith((ids) =>
                                    ids.includes(u.id) ? ids.filter((id) => id !== u.id) : [...ids, u.id],
                                  )
                                }
                                className="accent-accent"
                              />
                              <span className="truncate">{u.email}</span>
                            </label>
                          ))}
                        </div>
                      )}
                    </div>
                  )}
                </div>
              )}
            </div>
          ) : (
            <div>
              <div className="mb-3">
                <input
                  type="search"
                  value={filters.state.q}
                  onChange={(e) => filters.setQ(e.target.value)}
                  placeholder={t(`filters:search.${target === 'BOOK' ? 'books' : 'series'}`)}
                  className="h-8 w-full rounded-lg border border-line bg-surface px-3 text-sm text-ink transition-colors placeholder:text-ink-3 focus:border-accent/70 focus:outline-none"
                />
              </div>
              <div className="mb-3 flex items-center justify-between gap-2">
                <p className="text-xs text-ink-3">{count > 0 ? t('filtersActive', { count }) : t('filtersEmpty')}</p>
                <p className="flex items-center gap-1.5 text-xs text-ink-2" aria-live="polite">
                  {preview.isFetching && <CircleNotch className="size-3 animate-spin text-ink-3" />}
                  {preview.data && t(`count.${target === 'BOOK' ? 'book' : 'series'}`, { count: preview.data.totalElements })}
                </p>
              </div>
              {parsed.lossy && (
                <p className="mb-3 flex items-start gap-2 rounded-lg border border-warning/40 bg-warning-soft px-3 py-2 text-xs text-ink-2">
                  <WarningCircle className="mt-0.5 size-4 shrink-0 text-warning" />
                  {t('lossyWarning')}
                </p>
              )}
              {openDef ? (
                <FilterGroupDetailInline
                  def={openDef}
                  state={filters.state}
                  scope={scope}
                  onBack={() => setOpenGroup(null)}
                  onToggleValue={filters.toggleValue}
                  onToggleAuthor={filters.toggleAuthor}
                  onSetMode={filters.setMode}
                  onSetNegated={filters.setNegated}
                  onSetExclusive={filters.setExclusive}
                />
              ) : (
                <>
                  {enumGroups.map((def) => (
                    <EnumFilterSection
                      key={def.key}
                      def={def}
                      state={filters.state}
                      onToggleValue={filters.toggleValue}
                      onSetMode={filters.setMode}
                      onSetNegated={filters.setNegated}
                    />
                  ))}
                  {flagGroups.length > 0 && (
                    <section className="border-b border-line py-4">
                      <h3 className="mb-1 text-[11px] font-medium tracking-[0.08em] text-ink-3 uppercase">
                        {t('filters:section.flags')}
                      </h3>
                      {flagGroups.map((def) => (
                        <FlagFilterRow
                          key={def.key}
                          def={def}
                          value={(filters.state[def.key] as string[])[0]}
                          onCycle={() => filters.cycleValue(def.key, (def.options ?? []).map((o) => o.value))}
                        />
                      ))}
                    </section>
                  )}
                  {letterGroups.length > 0 && (
                    <section className="border-b border-line py-4">
                      {letterGroups.map((def) => (
                        <FilterGroupRow key={def.key} def={def} state={filters.state} labelFor={labelFor} onOpen={() => setOpenGroup(def.key)} />
                      ))}
                    </section>
                  )}
                  {navGroups.length > 0 && (
                    <section className="py-4">
                      <h3 className="mb-1 text-[11px] font-medium tracking-[0.08em] text-ink-3 uppercase">
                        {t('filters:section.metadata')}
                      </h3>
                      {navGroups.map((def) => (
                        <FilterGroupRow key={def.key} def={def} state={filters.state} labelFor={labelFor} onOpen={() => setOpenGroup(def.key)} />
                      ))}
                    </section>
                  )}
                </>
              )}
            </div>
          )}
        </div>

        <div className="flex shrink-0 items-center justify-between gap-2 border-t border-line px-5 py-3">
          {step === 1 ? (
            <>
              <span />
              <div className="flex gap-2">
                <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
                  {t('common:action.cancel')}
                </Button>
                <Button type="submit" variant="primary" disabled={!name.trim()}>
                  {t('next')}
                  <CaretRight className="size-4" />
                </Button>
              </div>
            </>
          ) : (
            <>
              <Button type="button" variant="ghost" onClick={() => setStep(1)}>
                <CaretLeft className="size-4" />
                {t('common:action.back')}
              </Button>
              <div className="flex items-center gap-2">
                {submitError && <span className="max-w-64 truncate text-xs text-danger">{submitError}</span>}
                <Button type="submit" variant="primary" loading={mutation.isPending} disabled={!name.trim()}>
                  {t('common:action.save')}
                </Button>
              </div>
            </>
          )}
        </div>
      </form>
    </Dialog>
  )
}

/** drill-down for one group inside the wizard: header mirrors the drawer's, content reuses FilterGroupDetail */
function FilterGroupDetailInline({
  def,
  state,
  scope,
  onBack,
  onToggleValue,
  onToggleAuthor,
  onSetMode,
  onSetNegated,
  onSetExclusive,
}: {
  def: FilterGroupDef
  state: FilterState
  scope?: ReferentialScope
  onBack: () => void
  onToggleValue: (key: GroupKey, value: string) => void
  onToggleAuthor: (author: { name: string; role: string }) => void
  onSetMode: (key: GroupKey, mode: 'any' | 'all') => void
  onSetNegated: (key: GroupKey, negated: boolean) => void
  onSetExclusive: (key: GroupKey, value: string) => void
}) {
  const { t } = useTranslation(['filters', 'smartlists'])
  return (
    <div>
      <div className="mb-3 flex items-center gap-1">
        <IconButton label={t('common:action.back')} onClick={onBack} className="-ml-2">
          <CaretLeft className="size-5" />
        </IconButton>
        <h3 className="min-w-0 flex-1 truncate font-display text-lg font-semibold text-ink">{t(def.labelKey)}</h3>
      </div>
      <FilterGroupDetail
        def={def}
        state={state}
        scope={scope}
        enabled
        onToggleValue={onToggleValue}
        onToggleAuthor={onToggleAuthor}
        onSetMode={onSetMode}
        onSetNegated={onSetNegated}
        onSetExclusive={onSetExclusive}
      />
    </div>
  )
}
