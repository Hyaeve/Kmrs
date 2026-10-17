import { useCallback, useEffect, useMemo } from 'react'
import { useSearchParams } from 'react-router-dom'
import i18n from '@/lib/i18n'
import { useLibraryPrefs } from '@/lib/store/libraryPrefs'
import { entityKindLabel } from '@/lib/utils/notableEntities'
import { AGE_RATING_UNSET, NOTABLE_ENTITY_ANYKIND, REFERENTIAL_ANY, type AuthorFilter, type FilterGroupDef, type FilterState, type GroupKey, type GroupMode, type NotableEntityFilter } from './types'

/** multi-value URL params, one key per filter group (values repeat the key) */
const MULTI_KEYS: GroupKey[] = [
  'readStatus',
  'seriesStatus',
  'complete',
  'oneshot',
  'deleted',
  'letter',
  'publishers',
  'genres',
  'tags',
  'sharingLabels',
  'ageRatings',
  'languages',
  'releaseYears',
  'authors',
  'notableEntities',
  'mediaProfiles',
  'mediaStatuses',
  'poster',
  'libraries',
  'readlists',
  'collections',
]

/** the persistable slice of a browse URL: q stays out so a remembered filter never brings back an old search text */
const PERSISTED_KEYS = [...MULTI_KEYS, 'ma', 'not']

function hasFilterParams(params: URLSearchParams): boolean {
  return PERSISTED_KEYS.some((k) => params.has(k))
}

function extractFilterParams(params: URLSearchParams): string {
  const out = new URLSearchParams()
  for (const k of PERSISTED_KEYS) for (const v of params.getAll(k)) out.append(k, v)
  return out.toString()
}

// names may contain commas, so the role is split off at the last comma; a per-entry negation is a
// '!' prefix on the serialized form (same convention as notable entities; a name starting with '!'
// would not survive)
export function parseAuthor(raw: string): AuthorFilter {
  const exclude = raw.startsWith('!')
  const s = exclude ? raw.slice(1) : raw
  const i = s.lastIndexOf(',')
  const base: AuthorFilter = i < 0 ? { name: s, role: '' } : { name: s.slice(0, i), role: s.slice(i + 1) }
  return exclude ? { ...base, exclude: true } : base
}

export function serializeAuthor(a: AuthorFilter): string {
  return `${a.exclude ? '!' : ''}${a.name},${a.role}`
}

/** chip and group-summary label: "Xu (writer)", "Any (colorist)", "None (editor)" */
export function authorChipLabel(a: AuthorFilter): string {
  const base = a.name || i18n.t(`filters:option.${a.exclude ? 'none' : 'any'}`)
  return a.role ? `${base} (${a.role})` : base
}

// a per-value negation is a '!' prefix on the serialized form; the kind is split off at the
// first comma, so names may contain commas (a kind with a comma of its own would not survive)
export function parseNotableEntity(raw: string): NotableEntityFilter {
  const exclude = raw.startsWith('!')
  const s = exclude ? raw.slice(1) : raw
  const i = s.indexOf(',')
  const base: NotableEntityFilter = i < 0 ? { kind: s, name: '' } : { kind: s.slice(0, i), name: s.slice(i + 1) }
  return exclude ? { ...base, exclude: true } : base
}

export function serializeNotableEntity(e: NotableEntityFilter): string {
  return `${e.exclude ? '!' : ''}${e.kind}${e.name ? `,${e.name}` : ''}`
}

export function parseFilterState(params: URLSearchParams): FilterState {
  return {
    q: params.get('q') ?? '',
    readStatus: params.getAll('readStatus'),
    seriesStatus: params.getAll('seriesStatus'),
    complete: params.getAll('complete'),
    oneshot: params.getAll('oneshot'),
    deleted: params.getAll('deleted'),
    letter: params.getAll('letter'),
    publishers: params.getAll('publishers'),
    genres: params.getAll('genres'),
    tags: params.getAll('tags'),
    sharingLabels: params.getAll('sharingLabels'),
    ageRatings: params.getAll('ageRatings'),
    languages: params.getAll('languages'),
    releaseYears: params.getAll('releaseYears'),
    // a fully empty entry (",") would only render as a phantom chip; the builder drops it anyway
    authors: params.getAll('authors').map(parseAuthor).filter((a) => a.name || a.role),
    notableEntities: params.getAll('notableEntities').map(parseNotableEntity),
    mediaProfiles: params.getAll('mediaProfiles'),
    mediaStatuses: params.getAll('mediaStatuses'),
    poster: params.getAll('poster'),
    libraries: params.getAll('libraries'),
    readlists: params.getAll('readlists'),
    collections: params.getAll('collections'),
    matchAll: params.getAll('ma').filter((v): v is GroupKey => (MULTI_KEYS as string[]).includes(v)),
    exclude: params.getAll('not').filter((v): v is GroupKey => (MULTI_KEYS as string[]).includes(v)),
  }
}

/** stable key for react-query caching; state derives from the URL so order is deterministic */
export function serializeFilters(s: FilterState): string {
  return JSON.stringify(s)
}

export function groupSelectedCount(def: FilterGroupDef, s: FilterState): number {
  if (def.kind === 'authors') return s.authors.length
  if (def.kind === 'notableEntities') return s.notableEntities.length
  return (s[def.key] as string[]).length
}

export function activeFilterCount(s: FilterState): number {
  return (
    s.readStatus.length +
    s.seriesStatus.length +
    s.complete.length +
    s.oneshot.length +
    s.deleted.length +
    s.letter.length +
    s.publishers.length +
    s.genres.length +
    s.tags.length +
    s.sharingLabels.length +
    s.ageRatings.length +
    s.languages.length +
    s.releaseYears.length +
    s.authors.length +
    s.notableEntities.length +
    s.mediaProfiles.length +
    s.mediaStatuses.length +
    s.poster.length +
    s.libraries.length +
    s.readlists.length +
    s.collections.length
  )
}

export interface ActiveFilterItem {
  key: string
  label: string
  group: GroupKey | 'q'
  /** raw URL value; for authors the serialized "name,role" form */
  value: string
}

// the sentinel is an English token baked into bookmarked URLs, so it has to go through i18n at display time
export function displayFilterValue(key: string, v: string): string {
  if (v === REFERENTIAL_ANY) return i18n.t('filters:option.any')
  return key === 'ageRatings' && v === AGE_RATING_UNSET ? i18n.t('filters:option.ageRating.unset') : v
}

/** known kinds get a translated label; custom analyzer kinds fall back to their raw token */
export function notableEntityKindLabel(kind: string): string {
  if (kind === NOTABLE_ENTITY_ANYKIND) return i18n.t('filters:notableEntities.anyKind')
  return entityKindLabel(kind)
}

/** chip and group-summary label: "Team: Endless", "Team: Any", "Location: not Hell's Kitchen" */
export function notableEntityChipLabel(e: NotableEntityFilter): string {
  const kind = notableEntityKindLabel(e.kind)
  // a nameless entry is the any/none pseudo-value; "none" already carries the negation
  if (!e.name) return i18n.t('filters:chip.notableEntity', { kind, name: i18n.t(`filters:notableEntities.${e.exclude ? 'none' : 'any'}`) })
  return i18n.t('filters:chip.notableEntity', { context: e.exclude ? 'negated' : undefined, kind, name: e.name })
}

export function describeActiveFilters(state: FilterState, groups: FilterGroupDef[]): ActiveFilterItem[] {
  const items: ActiveFilterItem[] = []
  for (const def of groups) {
    if (def.kind === 'authors') {
      for (const a of state.authors) {
        items.push({
          key: `authors:${serializeAuthor(a)}`,
          // a negated named entry spells "Authors: not X (role)"; the role pseudo self-negates ("None (role)")
          label:
            a.name && a.exclude
              ? i18n.t('filters:chip.value', { context: 'negated', group: i18n.t(def.labelKey), value: authorChipLabel(a) })
              : i18n.t('filters:chip.author', { name: authorChipLabel(a) }),
          group: 'authors',
          value: serializeAuthor(a),
        })
      }
      continue
    }
    if (def.kind === 'notableEntities') {
      for (const e of state.notableEntities) {
        items.push({
          key: `notableEntities:${serializeNotableEntity(e)}`,
          label: notableEntityChipLabel(e),
          group: 'notableEntities',
          value: serializeNotableEntity(e),
        })
      }
      continue
    }
    const negated = state.exclude.includes(def.key)
    const values = state[def.key] as string[]
    for (const v of values) {
      const labelKey = def.options?.find((o) => o.value === v)?.labelKey
      // the any/none pseudo spells its own negation; wrapping it in "not" would double-negate
      const selfNegating = v === REFERENTIAL_ANY
      items.push({
        key: `${def.key}:${v}`,
        label: i18n.t('filters:chip.value', {
          context: negated && !selfNegating ? 'negated' : undefined,
          group: i18n.t(def.labelKey),
          value: selfNegating && negated ? i18n.t('filters:option.none') : labelKey ? i18n.t(labelKey) : displayFilterValue(def.key, v),
        }),
        group: def.key,
        value: v,
      })
    }
  }
  if (state.q.trim()) items.push({ key: 'q', label: i18n.t('filters:chip.search', { q: state.q }), group: 'q', value: state.q })
  return items
}

function toggleParam(params: URLSearchParams, key: string, value: string) {
  const all = params.getAll(key)
  params.delete(key)
  const next = all.includes(value) ? all.filter((v) => v !== value) : [...all, value]
  for (const v of next) params.append(key, v)
}

function dropParamValue(params: URLSearchParams, key: string, value: string) {
  const rest = params.getAll(key).filter((v) => v !== value)
  params.delete(key)
  for (const v of rest) params.append(key, v)
}

export function useBrowseFilters(scope?: string, disabledKeys: readonly GroupKey[] = []) {
  const [searchParams, setSearchParams] = useSearchParams()
  const remembered = useLibraryPrefs((s) => (scope ? s.filters[scope] : undefined))
  const setFilters = useLibraryPrefs((s) => s.setFilters)

  // stale links may carry params for groups this page doesn't offer; drop them once so they
  // can't silently filter (there is no UI to see or clear them)
  useEffect(() => {
    if (!disabledKeys.some((k) => searchParams.has(k))) return
    const next = new URLSearchParams(searchParams)
    for (const k of disabledKeys) {
      next.delete(k)
      dropParamValue(next, 'ma', k)
      dropParamValue(next, 'not', k)
    }
    setSearchParams(next, { replace: true })
  }, [searchParams, disabledKeys, setSearchParams])

  // a URL without filter params falls back to the remembered ones (mirroring the remembered
  // sort); a URL that already carries any — deep links from metadata chips — is used as-is
  // and never persisted
  const effectiveParams = useMemo(() => {
    if (!remembered || hasFilterParams(searchParams)) return searchParams
    const merged = new URLSearchParams(searchParams)
    for (const [k, v] of new URLSearchParams(remembered)) merged.append(k, v)
    return merged
  }, [searchParams, remembered])

  const state = useMemo(() => {
    const parsed = parseFilterState(effectiveParams)
    for (const k of disabledKeys) parsed[k] = []
    return parsed
  }, [effectiveParams, disabledKeys])

  const update = useCallback(
    (mutate: (params: URLSearchParams) => void, replace = false) => {
      const next = new URLSearchParams(effectiveParams)
      mutate(next)
      if (scope) setFilters(scope, extractFilterParams(next))
      setSearchParams(next, { replace })
    },
    [effectiveParams, scope, setFilters, setSearchParams],
  )

  const toggleValue = useCallback(
    (key: GroupKey, value: string) =>
      update((p) => {
        toggleParam(p, key, value)
        // an emptied group must not keep its negation flag around
        if (p.getAll(key).length === 0) dropParamValue(p, 'not', key)
      }),
    [update],
  )

  const toggleAuthor = useCallback(
    (author: AuthorFilter) =>
      update((p) => {
        const all = p.getAll('authors').map(parseAuthor)
        const same = (a: AuthorFilter) => a.name === author.name && a.role === author.role
        const cur = all.find(same)
        p.delete('authors')
        const next = all.filter((a) => !same(a))
        // the same include/exclude form toggles off; a different form replaces the entry, so a
        // tri-state chip can move include → exclude in one call
        if (!cur || !cur.exclude !== !author.exclude) next.push(author)
        for (const a of next) p.append('authors', serializeAuthor(a))
      }),
    [update],
  )

  const toggleNotableEntity = useCallback(
    (entity: NotableEntityFilter) =>
      update((p) => {
        const all = p.getAll('notableEntities').map(parseNotableEntity)
        const same = (e: NotableEntityFilter) => e.kind === entity.kind && e.name === entity.name
        const cur = all.find(same)
        p.delete('notableEntities')
        const next = all.filter((e) => !same(e))
        // the same include/exclude form toggles off; a different form replaces the entry, so a
        // tri-state chip can move include → exclude in one call
        if (!cur || !cur.exclude !== !entity.exclude) next.push(entity)
        for (const e of next) p.append('notableEntities', serializeNotableEntity(e))
      }),
    [update],
  )

  const setMode = useCallback(
    (key: GroupKey, mode: GroupMode) =>
      update((p) => {
        dropParamValue(p, 'ma', key)
        // 'any' is the default and stays out of the URL
        if (mode === 'all') p.append('ma', key)
      }),
    [update],
  )

  const setNegated = useCallback(
    (key: GroupKey, negated: boolean) =>
      update((p) => {
        dropParamValue(p, 'not', key)
        // non-negated is the default and stays out of the URL
        if (negated) p.append('not', key)
      }),
    [update],
  )

  /** single-select groups (e.g. first letter): re-picking the active value clears it */
  const setExclusive = useCallback(
    (key: GroupKey, value: string) =>
      update((p) => {
        const current = p.get(key)
        p.delete(key)
        if (current !== value) p.set(key, value)
      }),
    [update],
  )

  /** tri-state flags cycle through the option order: off → yes → no → off */
  const cycleValue = useCallback(
    (key: GroupKey, options: string[]) =>
      update((p) => {
        const current = p.get(key)
        p.delete(key)
        const next = options[current === null ? 0 : options.indexOf(current) + 1]
        if (next !== undefined) p.set(key, next)
      }),
    [update],
  )

  const clearGroup = useCallback(
    (key: GroupKey) =>
      update((p) => {
        p.delete(key)
        dropParamValue(p, 'ma', key)
        dropParamValue(p, 'not', key)
      }),
    [update],
  )

  const setQ = useCallback(
    (q: string, replace = true) =>
      update(
        (p) => {
          if (q.trim()) p.set('q', q)
          else p.delete('q')
        },
        replace,
      ),
    [update],
  )

  const clearAll = useCallback(
    () =>
      update((p) => {
        for (const k of MULTI_KEYS) p.delete(k)
        p.delete('ma')
        p.delete('not')
      }),
    [update],
  )

  return { state, toggleValue, toggleAuthor, toggleNotableEntity, setMode, setNegated, setExclusive, cycleValue, clearGroup, setQ, clearAll }
}
