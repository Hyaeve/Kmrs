// FilterState plumbing for smart-list editing: a state-backed variant of the URL-driven
// useBrowseFilters, plus the reverse conversion from a stored search document.
import { useCallback, useState } from 'react'
import type { BookSearch, ConditionGroup, ConditionLeaf, SearchCondition, SeriesSearch, SmartListTarget } from '@/lib/api/types'
import { AGE_RATING_UNSET, type AuthorFilter, type FilterState, type GroupKey, type GroupMode } from '@/components/filters/types'

export function emptyFilterState(): FilterState {
  return {
    q: '',
    readStatus: [],
    seriesStatus: [],
    complete: [],
    oneshot: [],
    deleted: [],
    letter: [],
    publishers: [],
    genres: [],
    tags: [],
    sharingLabels: [],
    ageRatings: [],
    languages: [],
    releaseYears: [],
    authors: [],
    mediaProfiles: [],
    mediaStatuses: [],
    poster: [],
    libraries: [],
    readlists: [],
    collections: [],
    matchAll: [],
    exclude: [],
  }
}

/** same callback surface as useBrowseFilters, but over a useState value (dialog-local) */
export function useFilterState(initial: FilterState = emptyFilterState()) {
  const [state, setState] = useState(initial)

  const toggleIn = useCallback((key: GroupKey, value: string) => {
    setState((s) => {
      const values = s[key] as string[]
      const next = values.includes(value) ? values.filter((v) => v !== value) : [...values, value]
      return {
        ...s,
        [key]: next,
        // an emptied group must not keep its negation flag around
        exclude: next.length === 0 ? s.exclude.filter((k) => k !== key) : s.exclude,
      }
    })
  }, [])

  const toggleAuthor = useCallback((author: AuthorFilter) => {
    setState((s) => {
      const same = (a: AuthorFilter) => a.name === author.name && a.role === author.role
      const exists = s.authors.some(same)
      return { ...s, authors: exists ? s.authors.filter((a) => !same(a)) : [...s.authors, author] }
    })
  }, [])

  const setMode = useCallback((key: GroupKey, mode: GroupMode) => {
    setState((s) => ({
      ...s,
      // 'any' is the default and stays out of the state
      matchAll: mode === 'all' ? [...new Set([...s.matchAll, key])] : s.matchAll.filter((k) => k !== key),
    }))
  }, [])

  const setNegated = useCallback((key: GroupKey, negated: boolean) => {
    setState((s) => ({
      ...s,
      exclude: negated ? [...new Set([...s.exclude, key])] : s.exclude.filter((k) => k !== key),
    }))
  }, [])

  const setExclusive = useCallback((key: GroupKey, value: string) => {
    setState((s) => ({ ...s, [key]: (s[key] as string[])[0] === value ? [] : [value] }))
  }, [])

  const cycleValue = useCallback((key: GroupKey, options: string[]) => {
    setState((s) => {
      const current = (s[key] as string[])[0]
      const next = options[current === undefined ? 0 : options.indexOf(current) + 1]
      return { ...s, [key]: next === undefined ? [] : [next] }
    })
  }, [])

  const clearGroup = useCallback((key: GroupKey) => {
    setState((s) => ({ ...s, [key]: [], matchAll: s.matchAll.filter((k) => k !== key), exclude: s.exclude.filter((k) => k !== key) }))
  }, [])

  const setQ = useCallback((q: string) => setState((s) => ({ ...s, q })), [])
  const clearAll = useCallback(() => setState(emptyFilterState()), [])
  const reset = useCallback((next: FilterState) => setState(next), [])

  return { state, toggleValue: toggleIn, toggleAuthor, setMode, setNegated, setExclusive, cycleValue, clearGroup, setQ, clearAll, reset }
}

// ---- stored search document -> FilterState ----

export interface ParsedSearch {
  state: FilterState
  /** true when the document contains constructs the group editor cannot represent */
  lossy: boolean
}

type LeafOp = { operator: string; value?: unknown; dateTime?: string }

function leafKeyAndOp(branch: SearchCondition): [string, LeafOp] | undefined {
  if ('allOf' in branch || 'anyOf' in branch) return undefined
  const entries = Object.entries(branch as ConditionLeaf)
  if (entries.length !== 1) return undefined
  const [key, op] = entries[0]
  if (!op || typeof op !== 'object' || !('operator' in op)) return undefined
  return [key, op as LeafOp]
}

/** a release-year branch is the exact after/before pair buildBookSearch emits */
function yearFromBranch(branch: SearchCondition): string | undefined {
  if (!('allOf' in branch) || !Array.isArray(branch.allOf) || branch.allOf.length !== 2) return undefined
  const [a, b] = branch.allOf.map(leafKeyAndOp)
  if (!a || !b || a[0] !== 'releaseDate' || b[0] !== 'releaseDate') return undefined
  const after = a[1].operator === 'after' ? a[1].dateTime : b[1].operator === 'after' ? b[1].dateTime : undefined
  const before = a[1].operator === 'before' ? a[1].dateTime : b[1].operator === 'before' ? b[1].dateTime : undefined
  if (!after || !before) return undefined
  const m = /^(\d{4})-(\d{2})-(\d{2})T/.exec(after)
  if (!m) return undefined
  // the after bound is Dec 31 of the previous year, the before bound Jan 1 of the next
  const year = Number(m[1]) + 1
  const expected = `${year - 1}-12-31T00:00:00Z`
  const expectedBefore = `${year + 1}-01-01T00:00:00Z`
  if (!after.startsWith(expected) || !before.startsWith(expectedBefore)) return undefined
  return String(year)
}

interface Bucket {
  values: string[]
  /** isNot seen on any leaf: the whole group becomes negated */
  negated: boolean
  mode: GroupMode
}

/** the filter group a condition key feeds, or undefined when the editor has no such group;
    membership leaves only exist on their target's side (collections hold series, read lists hold books) */
function groupKeyForCondition(key: string, target: SmartListTarget): GroupKey | undefined {
  switch (key) {
    case 'readStatus':
    case 'seriesStatus':
    case 'complete':
    case 'deleted':
    case 'ageRating':
    case 'mediaProfile':
    case 'mediaStatus':
    case 'poster':
      return { readStatus: 'readStatus', seriesStatus: 'seriesStatus', complete: 'complete', deleted: 'deleted', ageRating: 'ageRatings', mediaProfile: 'mediaProfiles', mediaStatus: 'mediaStatuses', poster: 'poster' }[key] as GroupKey
    case 'oneShot':
      return 'oneshot'
    case 'titleSort':
      return 'letter'
    case 'publisher':
    case 'genre':
    case 'tag':
    case 'sharingLabel':
    case 'language':
      return { publisher: 'publishers', genre: 'genres', tag: 'tags', sharingLabel: 'sharingLabels', language: 'languages' }[key] as GroupKey
    case 'author':
      return 'authors'
    case 'libraryId':
      return 'libraries'
    case 'readListId':
      return target === 'BOOK' ? 'readlists' : undefined
    case 'collectionId':
      return target === 'SERIES' ? 'collections' : undefined
    default:
      return undefined
  }
}

/**
 * Reverses buildBookSearch/buildSeriesSearch for the subset the group editor can express:
 * a top-level leaf per group, same-key leaves combined as anyOf/allOf branches, the
 * library/read-list/collection membership anyOf, and canonical release-year pairs.
 * Anything else (nested logic, mixed is/isNot per group, regex-era shapes) sets `lossy`.
 */
export function searchToFilterState(search: BookSearch | SeriesSearch, target: SmartListTarget): ParsedSearch {
  const state = emptyFilterState()
  let lossy = false
  state.q = search.fullTextSearch ?? ''

  const buckets = new Map<GroupKey, Bucket>()
  const bucket = (key: GroupKey): Bucket => {
    let b = buckets.get(key)
    if (!b) {
      b = { values: [], negated: false, mode: 'any' }
      buckets.set(key, b)
    }
    return b
  }

  const absorbLeaf = (key: string, op: LeafOp): boolean => {
    // a group is uniformly is or isNot; a mixed leaf would silently flip the whole group's semantics
    const absorbIsValue = (group: GroupKey, value: string, isNot: boolean): boolean => {
      const b = bucket(group)
      if (b.values.length > 0 && b.negated !== isNot) return false
      if (isNot) b.negated = true
      b.values.push(value)
      return true
    }
    switch (key) {
      case 'readStatus':
        if (op.operator !== 'is' && op.operator !== 'isNot') return false
        if (op.value !== 'READ' && op.value !== 'UNREAD' && op.value !== 'IN_PROGRESS') return false
        return absorbIsValue('readStatus', op.value as string, op.operator === 'isNot')
      case 'seriesStatus':
        if (op.operator !== 'is' || typeof op.value !== 'string') return false
        if (!['ONGOING', 'ENDED', 'ABANDONED', 'HIATUS'].includes(op.value)) return false
        bucket('seriesStatus').values.push(op.value)
        return true
      case 'complete':
      case 'oneShot':
      case 'deleted': {
        if (op.operator !== 'isTrue' && op.operator !== 'isFalse') return false
        const group = key === 'oneShot' ? 'oneshot' : key
        bucket(group).values.push(op.operator === 'isTrue' ? 'true' : 'false')
        return true
      }
      case 'titleSort':
        if (op.operator !== 'beginsWith' || typeof op.value !== 'string' || op.value.length !== 1) return false
        bucket('letter').values.push(op.value)
        return true
      case 'publisher':
      case 'genre':
      case 'tag':
      case 'sharingLabel':
      case 'language': {
        if (op.operator !== 'is' && op.operator !== 'isNot') return false
        if (typeof op.value !== 'string') return false
        const group = groupKeyForCondition(key, target)
        if (!group) return false
        return absorbIsValue(group, op.value, op.operator === 'isNot')
      }
      case 'ageRating':
        if (op.operator === 'isNull') {
          bucket('ageRatings').values.push(AGE_RATING_UNSET)
          return true
        }
        if (op.operator === 'is' && (typeof op.value === 'string' || typeof op.value === 'number')) {
          bucket('ageRatings').values.push(String(op.value))
          return true
        }
        return false
      case 'mediaProfile':
        if (op.operator !== 'is' || typeof op.value !== 'string') return false
        bucket('mediaProfiles').values.push(op.value)
        return true
      case 'mediaStatus':
        if (op.operator !== 'is' || typeof op.value !== 'string') return false
        bucket('mediaStatuses').values.push(op.value)
        return true
      case 'poster': {
        const selected = op.value !== undefined && (op.value as { selected?: boolean }).selected === true
        if (op.operator === 'is' && selected) bucket('poster').values.push('selected')
        else if (op.operator === 'isNot' && selected) bucket('poster').values.push('missing')
        else return false
        return true
      }
      case 'author': {
        if (op.operator !== 'is' || typeof op.value !== 'object' || op.value === null) return false
        const a = op.value as { name?: unknown; role?: unknown }
        if (typeof a.name !== 'string') return false
        state.authors.push({ name: a.name, role: typeof a.role === 'string' ? a.role : '' })
        // marker for mode-conflict detection; state.authors carries the real values
        bucket('authors').values.push(a.name)
        return true
      }
      case 'libraryId':
      case 'readListId':
      case 'collectionId': {
        if (op.operator !== 'is' && op.operator !== 'isNot') return false
        if (typeof op.value !== 'string') return false
        const group = groupKeyForCondition(key, target)
        if (!group) return false
        return absorbIsValue(group, op.value, op.operator === 'isNot')
      }
      default:
        return false
    }
  }

  const condition = search.condition as ConditionGroup | ConditionLeaf | undefined
  // a single-group document is the group's own shape, not a container of branches:
  // the whole condition is one canonical year pair, an anyOf/allOf of same-key leaves
  // (match-any/match-all group), or an anyOf/allOf run of year pairs
  let branches: SearchCondition[] = []
  if (condition) {
    if (yearFromBranch(condition) !== undefined) {
      branches = [condition as SearchCondition]
    } else {
      const members = (condition as ConditionGroup).allOf ?? (condition as ConditionGroup).anyOf
      if (Array.isArray(members) && members.length > 0) {
        const leafKeys = members.map((m) => leafKeyAndOp(m)?.[0])
        const singleGroup =
          leafKeys.every((k) => k !== undefined && k === leafKeys[0]) ||
          members.every((m) => yearFromBranch(m) !== undefined)
        branches = singleGroup ? [condition as SearchCondition] : members
      } else {
        branches = [condition as SearchCondition]
      }
    }
  }

  const seenSingleKeys = new Set<string>()
  for (const branch of branches) {
    const year = yearFromBranch(branch)
    if (year !== undefined) {
      bucket('releaseYears').values.push(year)
      continue
    }
    const single = leafKeyAndOp(branch)
    if (single) {
      // the builder wraps multi-value groups, so a repeated bare key means a hand-crafted
      // same-key run — parsing it as match-any would silently change AND into OR
      if (seenSingleKeys.has(single[0])) lossy = true
      seenSingleKeys.add(single[0])
      if (!absorbLeaf(single[0], single[1])) lossy = true
      continue
    }
    // a group branch: same-key leaves combined with anyOf (or allOf = match-all mode)
    const group: SearchCondition[] | undefined = (branch as ConditionGroup).anyOf ?? (branch as ConditionGroup).allOf
    if (!Array.isArray(group) || group.length === 0) {
      lossy = true
      continue
    }
    const mode: GroupMode = 'allOf' in branch ? 'all' : 'any'
    // a run of canonical release-year pairs is the release-year group
    const years = group.map(yearFromBranch)
    if (years.every((y) => y !== undefined)) {
      const b = bucket('releaseYears')
      if (b.values.length > 0 && b.mode !== mode) lossy = true
      b.mode = mode
      for (const y of years as string[]) b.values.push(y)
      continue
    }
    const leaves = group.map(leafKeyAndOp)
    const key = leaves[0]?.[0]
    if (!key || leaves.some((l) => !l || l[0] !== key)) {
      lossy = true
      continue
    }
    // mode and values must land on the same group: bucket through the mapping absorbLeaf uses
    const groupKey = groupKeyForCondition(key, target)
    if (!groupKey) {
      lossy = true
      continue
    }
    const b = bucket(groupKey)
    if (b.values.length > 0 && b.mode !== mode) lossy = true
    b.mode = mode
    for (const [, op] of leaves as [string, LeafOp][]) {
      if (!absorbLeaf(key, op)) lossy = true
    }
  }

  for (const [key, b] of buckets) {
    // authors are structured objects already collected in state.authors; the bucket only tracks the mode
    if (key !== 'authors') (state[key] as string[]) = [...new Set(b.values)]
    if (b.negated) state.exclude = [...state.exclude, key]
    if (b.mode === 'all') state.matchAll = [...state.matchAll, key]
  }
  return { state, lossy }
}
