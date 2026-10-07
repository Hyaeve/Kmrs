import { useCallback, useEffect, useMemo } from 'react'
import { useSearchParams } from 'react-router-dom'
import i18n from '@/lib/i18n'
import { AGE_RATING_UNSET, type AuthorFilter, type FilterGroupDef, type FilterState, type GroupKey, type GroupMode } from './types'

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
  'mediaProfiles',
  'mediaStatuses',
  'poster',
  'libraries',
  'readlists',
  'collections',
]

// names may contain commas, so the role is split off at the last comma
export function parseAuthor(raw: string): AuthorFilter {
  const i = raw.lastIndexOf(',')
  return i < 0 ? { name: raw, role: '' } : { name: raw.slice(0, i), role: raw.slice(i + 1) }
}

export function serializeAuthor(a: AuthorFilter): string {
  return `${a.name},${a.role}`
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
    authors: params.getAll('authors').map(parseAuthor),
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
  return def.kind === 'authors' ? s.authors.length : (s[def.key] as string[]).length
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
  return key === 'ageRatings' && v === AGE_RATING_UNSET ? i18n.t('filters:option.ageRating.unset') : v
}

export function describeActiveFilters(state: FilterState, groups: FilterGroupDef[]): ActiveFilterItem[] {
  const items: ActiveFilterItem[] = []
  for (const def of groups) {
    if (def.kind === 'authors') {
      for (const a of state.authors) {
        items.push({
          key: `authors:${serializeAuthor(a)}`,
          label: i18n.t('filters:chip.author', { name: a.role ? `${a.name} (${a.role})` : a.name }),
          group: 'authors',
          value: serializeAuthor(a),
        })
      }
      continue
    }
    const negated = state.exclude.includes(def.key)
    const values = state[def.key] as string[]
    for (const v of values) {
      const labelKey = def.options?.find((o) => o.value === v)?.labelKey
      items.push({
        key: `${def.key}:${v}`,
        label: i18n.t('filters:chip.value', {
          context: negated ? 'negated' : undefined,
          group: i18n.t(def.labelKey),
          value: labelKey ? i18n.t(labelKey) : displayFilterValue(def.key, v),
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

export function useBrowseFilters(disabledKeys: readonly GroupKey[] = []) {
  const [searchParams, setSearchParams] = useSearchParams()

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

  const state = useMemo(() => {
    const parsed = parseFilterState(searchParams)
    for (const k of disabledKeys) parsed[k] = []
    return parsed
  }, [searchParams, disabledKeys])

  const update = useCallback(
    (mutate: (params: URLSearchParams) => void, replace = false) => {
      const next = new URLSearchParams(searchParams)
      mutate(next)
      setSearchParams(next, { replace })
    },
    [searchParams, setSearchParams],
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
    (author: AuthorFilter) => update((p) => toggleParam(p, 'authors', serializeAuthor(author))),
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

  return { state, toggleValue, toggleAuthor, setMode, setNegated, setExclusive, cycleValue, clearGroup, setQ, clearAll }
}
