import { useEffect, useMemo, useState, type ReactNode } from 'react'
import { keepPreviousData, useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { CaretRight, CheckCircle, Circle, X, XCircle } from '@phosphor-icons/react'
import { collectionsApi, readlistsApi } from '@/lib/api/collections'
import { librariesApi } from '@/lib/api/libraries'
import { referentialApi, type ReferentialScope } from '@/lib/api/referential'
import { Skeleton } from '@/components/ui/Skeleton'
import { SegmentedControl } from '@/components/ui/SegmentedControl'
import { cn } from '@/lib/utils/cn'
import { displayFilterValue, groupSelectedCount, notableEntityChipLabel, notableEntityKindLabel } from './filterUrl'
import { AGE_RATING_UNSET, LETTERS, NOTABLE_ENTITY_ANYKIND, type AuthorFilter, type FilterGroupDef, type FilterState, type GroupKey, type GroupMode, type NotableEntityFilter, type ReferentialKind } from './types'

const MODE_OPTIONS: { value: GroupMode; labelKey: string }[] = [
  { value: 'any', labelKey: 'filters:mode.any' },
  { value: 'all', labelKey: 'filters:mode.all' },
]

const NEGATE_OPTIONS: { value: 'is' | 'isNot'; labelKey: string }[] = [
  { value: 'is', labelKey: 'filters:negate.is' },
  { value: 'isNot', labelKey: 'filters:negate.isNot' },
]

const GROUP_SEARCH_THRESHOLD = 40

function OptionChip({ active, excluded, onClick, children }: { active?: boolean; excluded?: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        'inline-flex cursor-pointer items-center gap-1 rounded-full border px-2.5 py-1 text-xs transition-colors',
        excluded
          ? 'border-danger/50 bg-danger/10 text-danger'
          : active
            ? 'border-accent/50 bg-accent-soft text-accent-strong'
            : 'border-line bg-raised text-ink-2 hover:border-line-strong hover:text-ink',
      )}
    >
      {children}
    </button>
  )
}

function GroupSearchInput({ value, onChange, placeholder }: { value: string; onChange: (v: string) => void; placeholder: string }) {
  return (
    <input
      type="search"
      value={value}
      onChange={(e) => onChange(e.target.value)}
      placeholder={placeholder}
      className="mb-2 h-8 w-full rounded-lg border border-line bg-surface px-2.5 text-base text-ink transition-colors placeholder:text-ink-3 focus:border-accent/70 focus:outline-none"
    />
  )
}

function LoadingChips() {
  return (
    <div className="flex flex-wrap gap-1.5">
      <Skeleton className="h-6 w-16 rounded-full" />
      <Skeleton className="h-6 w-24 rounded-full" />
      <Skeleton className="h-6 w-20 rounded-full" />
    </div>
  )
}

function LoadError({ onRetry }: { onRetry: () => void }) {
  const { t } = useTranslation('filters')
  return (
    <p className="text-xs text-ink-3">
      {t('options.loadError')}{' '}
      <button type="button" onClick={onRetry} className="cursor-pointer text-accent-strong hover:underline">
        {t('common:action.retry')}
      </button>
    </p>
  )
}

function fetchReferential(kind: ReferentialKind, scope?: ReferentialScope): Promise<string[]> {
  switch (kind) {
    case 'publishers':
      return referentialApi.publishers(scope)
    case 'genres':
      return referentialApi.genres(scope)
    case 'tags':
      return referentialApi.tags(scope)
    case 'bookTags':
      return referentialApi.bookTags(scope)
    case 'sharingLabels':
      return referentialApi.sharingLabels(scope)
    case 'ageRatings':
      return referentialApi.ageRatings(scope)
    case 'languages':
      return referentialApi.languages(scope)
    case 'releaseDates':
      return referentialApi.releaseDates(scope)
  }
}

function sortReferential(kind: ReferentialKind, values: string[], locale: string): string[] {
  const arr = [...values]
  if (kind === 'releaseDates') return arr.sort((a, b) => Number(b) - Number(a))
  if (kind === 'ageRatings')
    return arr.sort((a, b) => {
      const na = Number(a)
      const nb = Number(b)
      if (!Number.isNaN(na) && !Number.isNaN(nb)) return na - nb
      return a.localeCompare(b, locale)
    })
  return arr.sort((a, b) => a.localeCompare(b, locale))
}

function ReferentialOptions({
  kind,
  scope,
  selected,
  enabled,
  onToggle,
}: {
  kind: ReferentialKind
  scope?: ReferentialScope
  selected: string[]
  enabled: boolean
  onToggle: (value: string) => void
}) {
  const [filter, setFilter] = useState('')
  const { t, i18n } = useTranslation('filters')
  const query = useQuery({
    queryKey: ['referential', kind, scope],
    queryFn: () => fetchReferential(kind, scope),
    enabled,
    staleTime: 60_000,
  })

  const options = useMemo(() => {
    const sorted = sortReferential(kind, query.data ?? [], i18n.language)
    // the v2 endpoint drops NULL age ratings, so the unset value is offered as a fixed option,
    // listed first like the v1 endpoint served it
    return kind === 'ageRatings' ? [AGE_RATING_UNSET, ...sorted] : sorted
  }, [kind, query.data, i18n.language])
  const visible = useMemo(() => {
    const f = filter.trim().toLowerCase()
    // selected values stay visible so they can be deselected while filtering
    return options.filter((o) => selected.includes(o) || !f || displayFilterValue(kind, o).toLowerCase().includes(f))
  }, [options, filter, selected, kind])

  if (query.isPending) return <LoadingChips />
  if (query.isLoadingError) return <LoadError onRetry={() => query.refetch()} />

  return (
    <div>
      {options.length > GROUP_SEARCH_THRESHOLD && (
        <GroupSearchInput value={filter} onChange={setFilter} placeholder={t('options.filterPlaceholder')} />
      )}
      {visible.length === 0 ? (
        <p className="text-xs text-ink-3">{t('options.noMatch')}</p>
      ) : (
        <div className="flex flex-wrap gap-1.5">
          {visible.map((o) => (
            <OptionChip key={o} active={selected.includes(o)} onClick={() => onToggle(o)}>
              {displayFilterValue(kind, o)}
            </OptionChip>
          ))}
        </div>
      )}
    </div>
  )
}

interface EntityOption {
  id: string
  name: string
}

function fetchEntities(kind: NonNullable<FilterGroupDef['entities']>): Promise<EntityOption[]> {
  if (kind === 'libraries') return librariesApi.list()
  if (kind === 'collections') return collectionsApi.list({ unpaged: true }).then((p) => p.content)
  return readlistsApi.list({ unpaged: true }).then((p) => p.content)
}

/** id→name options for entity groups (libraries, read lists, collections); the search DSL
    stores ids, so names are resolved here for display and by the caller for chip labels */
export function EntityOptions({
  kind,
  selected,
  enabled,
  onToggle,
}: {
  kind: NonNullable<FilterGroupDef['entities']>
  selected: string[]
  enabled: boolean
  onToggle: (id: string) => void
}) {
  const [filter, setFilter] = useState('')
  const { t, i18n } = useTranslation('filters')
  const query = useQuery({
    queryKey: ['entity-options', kind],
    queryFn: () => fetchEntities(kind),
    enabled,
    staleTime: 60_000,
  })
  const names = useMemo(() => new Map((query.data ?? []).map((e) => [e.id, e.name])), [query.data])
  const options = useMemo(
    () => [...names.keys()].sort((a, b) => (names.get(a) ?? a).localeCompare(names.get(b) ?? b, i18n.language)),
    [names, i18n.language],
  )
  const visible = useMemo(() => {
    const f = filter.trim().toLowerCase()
    return options.filter((o) => selected.includes(o) || !f || (names.get(o) ?? o).toLowerCase().includes(f))
  }, [options, filter, selected, names])

  if (query.isPending) return <LoadingChips />
  if (query.isLoadingError) return <LoadError onRetry={() => query.refetch()} />

  return (
    <div>
      {options.length > GROUP_SEARCH_THRESHOLD && (
        <GroupSearchInput value={filter} onChange={setFilter} placeholder={t('options.filterPlaceholder')} />
      )}
      {visible.length === 0 ? (
        <p className="text-xs text-ink-3">{t('options.noMatch')}</p>
      ) : (
        <div className="flex flex-wrap gap-1.5">
          {visible.map((id) => (
            <OptionChip key={id} active={selected.includes(id)} onClick={() => onToggle(id)}>
              {names.get(id) ?? id}
              {selected.includes(id) && <X className="size-3" />}
            </OptionChip>
          ))}
        </div>
      )}
    </div>
  )
}

function LetterOptions({ selected, onSelect }: { selected: string[]; onSelect: (letter: string) => void }) {
  const active = selected[0]
  return (
    <div className="flex flex-wrap gap-1">
      {[...LETTERS, '#'].map((l) => (
        <button
          key={l}
          type="button"
          onClick={() => onSelect(l)}
          aria-pressed={active === l}
          className={cn(
            'size-7 cursor-pointer rounded-md font-mono text-xs transition-colors',
            active === l ? 'bg-accent-soft text-accent-strong' : 'text-ink-3 hover:bg-raised hover:text-ink',
          )}
        >
          {l}
        </button>
      ))}
    </div>
  )
}

function authorLabel(a: AuthorFilter): string {
  return a.role ? `${a.name} (${a.role})` : a.name
}

function AuthorsOptions({
  scope,
  selected,
  enabled,
  onToggle,
}: {
  scope?: ReferentialScope
  selected: AuthorFilter[]
  enabled: boolean
  onToggle: (author: AuthorFilter) => void
}) {
  const [filter, setFilter] = useState('')
  const { t, i18n } = useTranslation('filters')
  const query = useQuery({
    queryKey: ['referential', 'authors', scope],
    queryFn: () => referentialApi.authors(scope),
    enabled,
    staleTime: 60_000,
  })

  const same = (a: AuthorFilter, b: AuthorFilter) => a.name === b.name && a.role === b.role
  const options = useMemo(() => {
    const arr = [...(query.data ?? [])]
    arr.sort((a, b) => authorLabel(a).localeCompare(authorLabel(b), i18n.language))
    return arr
  }, [query.data, i18n.language])
  const visible = useMemo(() => {
    const f = filter.trim().toLowerCase()
    // selected values stay visible so they can be deselected while filtering
    return options.filter((o) => selected.some((s) => same(o, s)) || !f || authorLabel(o).toLowerCase().includes(f))
  }, [options, filter, selected])

  return (
    <div>
      {options.length > GROUP_SEARCH_THRESHOLD && (
        <GroupSearchInput value={filter} onChange={setFilter} placeholder={t('authors.searchPlaceholder')} />
      )}
      {query.isPending ? (
        <LoadingChips />
      ) : query.isLoadingError ? (
        <LoadError onRetry={() => query.refetch()} />
      ) : visible.length === 0 ? (
        <p className="text-xs text-ink-3">{t('authors.noMatch')}</p>
      ) : (
        <div className="flex flex-wrap gap-1.5">
          {visible.map((a) => {
            const active = selected.some((s) => same(a, s))
            return (
              <OptionChip key={`${a.name},${a.role}`} active={active} onClick={() => onToggle({ name: a.name, role: a.role })}>
                {authorLabel(a)}
                {active && <X className="size-3" />}
              </OptionChip>
            )
          })}
        </div>
      )}
    </div>
  )
}

// known kinds lead in label-key order, custom analyzer kinds follow in server order
const KNOWN_ENTITY_KINDS = ['character', 'team', 'location']

/** name lists shorter than this get no search box (komga next-ui hides it under 10) */
const NAME_SEARCH_THRESHOLD = 10

function orderEntityKinds(server: string[]): string[] {
  const rank = (k: string) => {
    const i = KNOWN_ENTITY_KINDS.indexOf(k)
    return i === -1 ? KNOWN_ENTITY_KINDS.length : i
  }
  return [NOTABLE_ENTITY_ANYKIND, ...[...server].sort((a, b) => rank(a) - rank(b))]
}

function NotableEntitiesOptions({
  scope,
  selected,
  enabled,
  onToggle,
}: {
  scope?: ReferentialScope
  selected: NotableEntityFilter[]
  enabled: boolean
  onToggle: (entity: NotableEntityFilter) => void
}) {
  const query = useQuery({
    queryKey: ['referential', 'notableEntityKinds', scope],
    queryFn: () => referentialApi.notableEntityKinds(scope),
    enabled,
    staleTime: 60_000,
  })
  const kinds = useMemo(() => orderEntityKinds(query.data ?? []), [query.data])

  if (query.isPending) return <LoadingChips />
  if (query.isLoadingError) return <LoadError onRetry={() => query.refetch()} />

  return (
    <div>
      {kinds.map((kind) => (
        <NotableEntityKindPanel
          key={kind}
          kind={kind}
          scope={scope}
          selected={selected.filter((e) => e.kind === kind)}
          enabled={enabled}
          onToggle={onToggle}
        />
      ))}
    </div>
  )
}

function NotableEntityKindPanel({
  kind,
  scope,
  selected,
  enabled,
  onToggle,
}: {
  kind: string
  scope?: ReferentialScope
  /** entries of this kind only */
  selected: NotableEntityFilter[]
  enabled: boolean
  onToggle: (entity: NotableEntityFilter) => void
}) {
  const { t, i18n } = useTranslation('filters')
  const [search, setSearch] = useState('')
  const [debounced, setDebounced] = useState('')
  useEffect(() => {
    const id = setTimeout(() => setDebounced(search.trim()), 500)
    return () => clearTimeout(id)
  }, [search])
  const query = useQuery({
    queryKey: ['referential', 'notableEntityNames', kind, debounced, scope],
    queryFn: () => referentialApi.notableEntityNames(kind === NOTABLE_ENTITY_ANYKIND ? undefined : kind, debounced || undefined, scope),
    enabled,
    staleTime: 60_000,
    placeholderData: keepPreviousData,
  })

  // selected names stay visible so they can be cycled back while a search filters the list
  const names = useMemo(() => {
    const all = new Set([...selected.filter((e) => e.name !== '').map((e) => e.name), ...(query.data ?? [])])
    return [...all].sort((a, b) => a.localeCompare(b, i18n.language))
  }, [selected, query.data, i18n.language])
  const byName = useMemo(() => new Map(selected.map((e) => [e.name, e])), [selected])

  // onToggle replaces a different include/exclude form and removes the same one,
  // so one call steps the chip through off → include → exclude → off
  const cycle = (name: string) => {
    const cur = byName.get(name)
    if (!cur) onToggle({ kind, name })
    else if (!cur.exclude) onToggle({ ...cur, exclude: true })
    else onToggle(cur)
  }

  const pseudo = byName.get('')
  // the box must stay visible while a search is active, even if the filtered list is short
  const showSearch = search !== '' || (query.data?.length ?? 0) > NAME_SEARCH_THRESHOLD

  return (
    <section className="mt-5 first:mt-0">
      <h4 className="mb-2 text-xs font-medium text-ink-2">{notableEntityKindLabel(kind)}</h4>
      {showSearch && <GroupSearchInput value={search} onChange={setSearch} placeholder={t('notableEntities.searchPlaceholder')} />}
      {query.isPending ? (
        <LoadingChips />
      ) : query.isLoadingError ? (
        <LoadError onRetry={() => query.refetch()} />
      ) : (
        <div className="flex flex-wrap gap-1.5">
          <OptionChip active={!!pseudo && !pseudo.exclude} excluded={pseudo?.exclude} onClick={() => cycle('')}>
            {pseudo?.exclude ? t('notableEntities.none') : t('notableEntities.any')}
          </OptionChip>
          {names.map((name) => {
            const cur = byName.get(name)
            return (
              <OptionChip key={name} active={!!cur && !cur.exclude} excluded={cur?.exclude} onClick={() => cycle(name)}>
                {name}
              </OptionChip>
            )
          })}
          {names.length === 0 && debounced !== '' && <p className="text-xs text-ink-3">{t('notableEntities.noMatch')}</p>}
        </div>
      )}
    </section>
  )
}

interface ControlProps {
  def: FilterGroupDef
  state: FilterState
  onSetMode: (key: GroupKey, mode: GroupMode) => void
  onSetNegated: (key: GroupKey, negated: boolean) => void
  className?: string
}

/** per-group is/is-not and any/all switches; hidden while the group has too few values to need them */
function GroupControls({ def, state, onSetMode, onSetNegated, className }: ControlProps) {
  const { t } = useTranslation('filters')
  const count = groupSelectedCount(def, state)
  const mode: GroupMode = state.matchAll.includes(def.key) ? 'all' : 'any'
  const negated = state.exclude.includes(def.key)
  const showNegate = def.negatable && count >= 1
  const showMode = count >= 2
  if (!showNegate && !showMode) return null
  return (
    <div className={cn('flex items-center gap-1.5', className)}>
      {showNegate && (
        <SegmentedControl
          size="sm"
          options={NEGATE_OPTIONS.map((o) => ({ value: o.value, label: t(o.labelKey) }))}
          value={negated ? 'isNot' : 'is'}
          onChange={(m) => onSetNegated(def.key, m === 'isNot')}
        />
      )}
      {showMode && (
        <SegmentedControl
          size="sm"
          options={MODE_OPTIONS.map((o) => ({ value: o.value, label: t(o.labelKey) }))}
          value={mode}
          onChange={(m) => onSetMode(def.key, m)}
        />
      )}
    </div>
  )
}

interface EnumSectionProps {
  def: FilterGroupDef
  state: FilterState
  onToggleValue: (key: GroupKey, value: string) => void
  onSetMode: (key: GroupKey, mode: GroupMode) => void
  onSetNegated: (key: GroupKey, negated: boolean) => void
}

/** small enum groups stay inline in the main view as a chip row */
export function EnumFilterSection({ def, state, onToggleValue, onSetMode, onSetNegated }: EnumSectionProps) {
  const { t } = useTranslation('filters')
  const values = state[def.key] as string[]
  return (
    <section className="border-b border-line py-4">
      <div className="mb-2.5 flex flex-wrap items-center justify-between gap-x-2 gap-y-1.5">
        <h3 className="text-[11px] font-medium tracking-[0.08em] text-ink-3 uppercase">{t(def.labelKey)}</h3>
        <GroupControls def={def} state={state} onSetMode={onSetMode} onSetNegated={onSetNegated} />
      </div>
      <div className="flex flex-wrap gap-1.5">
        {(def.options ?? []).map((o) => (
          <OptionChip key={o.value} active={values.includes(o.value)} onClick={() => onToggleValue(def.key, o.value)}>
            {t(o.labelKey)}
          </OptionChip>
        ))}
      </div>
    </section>
  )
}

export function FlagFilterRow({ def, value, onCycle }: { def: FilterGroupDef; value?: string; onCycle: () => void }) {
  const { t } = useTranslation('filters')
  return (
    <button
      type="button"
      onClick={onCycle}
      aria-pressed={value !== undefined}
      className="flex w-full cursor-pointer items-center justify-between border-t border-line py-2.5 text-left first:border-t-0"
    >
      <span className="text-sm text-ink">{t(def.labelKey)}</span>
      {value === 'true' ? (
        <CheckCircle weight="fill" className="size-5 text-accent-strong" />
      ) : value === 'false' ? (
        <XCircle weight="fill" className="size-5 text-danger" />
      ) : (
        <Circle className="size-5 text-ink-3" />
      )}
    </button>
  )
}

function groupSummary(def: FilterGroupDef, state: FilterState, locale: string, labelFor?: (key: GroupKey, id: string) => string): string {
  const list = new Intl.ListFormat(locale, { style: 'narrow', type: 'conjunction' })
  if (def.kind === 'authors') return list.format(state.authors.map((a) => (a.role ? `${a.name} (${a.role})` : a.name)))
  if (def.kind === 'notableEntities') return list.format(state.notableEntities.map(notableEntityChipLabel))
  return list.format((state[def.key] as string[]).map((v) => labelFor?.(def.key, v) ?? displayFilterValue(def.key, v)))
}

/** collapsed row for high-cardinality groups; opens the group's detail view */
export function FilterGroupRow({
  def,
  state,
  labelFor,
  onOpen,
}: {
  def: FilterGroupDef
  state: FilterState
  labelFor?: (key: GroupKey, id: string) => string
  onOpen: () => void
}) {
  const { t, i18n } = useTranslation('filters')
  const summary = groupSummary(def, state, i18n.language, labelFor)
  return (
    <button
      type="button"
      onClick={onOpen}
      className="flex w-full cursor-pointer items-center justify-between gap-3 border-t border-line py-2.5 text-left first:border-t-0"
    >
      <span className="shrink-0 text-sm text-ink">{t(def.labelKey)}</span>
      <span className="flex min-w-0 items-center gap-1">
        {summary && <span className="truncate text-xs text-accent-strong">{summary}</span>}
        <CaretRight className="size-4 shrink-0 text-ink-3" />
      </span>
    </button>
  )
}

interface DetailProps {
  def: FilterGroupDef
  state: FilterState
  scope?: ReferentialScope
  enabled: boolean
  onToggleValue: (key: GroupKey, value: string) => void
  onToggleAuthor: (author: AuthorFilter) => void
  onToggleNotableEntity: (entity: NotableEntityFilter) => void
  onSetMode: (key: GroupKey, mode: GroupMode) => void
  onSetNegated: (key: GroupKey, negated: boolean) => void
  onSetExclusive: (key: GroupKey, value: string) => void
}

/** full editor for one group, shown in the drawer's detail view */
export function FilterGroupDetail({
  def,
  state,
  scope,
  enabled,
  onToggleValue,
  onToggleAuthor,
  onToggleNotableEntity,
  onSetMode,
  onSetNegated,
  onSetExclusive,
}: DetailProps) {
  // authors and notableEntities carry structured values consumed via `state` directly,
  // not the flat string list
  const values = def.kind === 'authors' || def.kind === 'notableEntities' ? [] : (state[def.key] as string[])
  return (
    <div className="py-4">
      <GroupControls def={def} state={state} onSetMode={onSetMode} onSetNegated={onSetNegated} className="mb-3" />
      {def.kind === 'referential' && def.referential && (
        <ReferentialOptions
          kind={def.referential}
          scope={scope}
          selected={values}
          enabled={enabled}
          onToggle={(v) => onToggleValue(def.key, v)}
        />
      )}
      {def.kind === 'entities' && def.entities && (
        <EntityOptions kind={def.entities} selected={values} enabled={enabled} onToggle={(v) => onToggleValue(def.key, v)} />
      )}
      {def.kind === 'letters' && <LetterOptions selected={values} onSelect={(l) => onSetExclusive(def.key, l)} />}
      {def.kind === 'authors' && (
        <AuthorsOptions scope={scope} selected={state.authors} enabled={enabled} onToggle={onToggleAuthor} />
      )}
      {def.kind === 'notableEntities' && (
        <NotableEntitiesOptions scope={scope} selected={state.notableEntities} enabled={enabled} onToggle={onToggleNotableEntity} />
      )}
    </div>
  )
}
