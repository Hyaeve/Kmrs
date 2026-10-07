import type { BookSearch, ConditionLeaf, SearchCondition, SearchOperator, SeriesSearch } from '@/lib/api/types'
import { AGE_RATING_UNSET, LETTERS, type AuthorFilter, type FilterState, type GroupKey, type GroupMode } from './types'

function leaf(key: string, operator: SearchOperator, value?: unknown): ConditionLeaf {
  return value === undefined ? { [key]: { operator } } : { [key]: { operator, value } }
}

function combine(conditions: SearchCondition[], mode: GroupMode): SearchCondition | undefined {
  if (conditions.length === 0) return undefined
  if (conditions.length === 1) return conditions[0]
  return mode === 'any' ? { anyOf: conditions } : { allOf: conditions }
}

function push(out: SearchCondition[], condition: SearchCondition | undefined) {
  if (condition) out.push(condition)
}

function yearConditions(years: string[]): SearchCondition[] {
  return years.map((y) => {
    const year = Number(y)
    // date ops compare strict against the UTC date, so the bounds sit just outside the year
    return {
      allOf: [
        { releaseDate: { operator: 'after', dateTime: `${year - 1}-12-31T00:00:00Z` } },
        { releaseDate: { operator: 'before', dateTime: `${year + 1}-01-01T00:00:00Z` } },
      ],
    }
  })
}

function authorConditions(authors: AuthorFilter[]): SearchCondition[] {
  return authors.map((a) => leaf('author', 'is', a.role ? { name: a.name, role: a.role } : { name: a.name }))
}

/** boolean DSL leaves carry no value, just isTrue/isFalse */
function boolLeaf(key: string, v: string): ConditionLeaf {
  return leaf(key, v === 'true' ? 'isTrue' : 'isFalse')
}

function letterCondition(letter: string[]): SearchCondition | undefined {
  const l = letter[0]
  if (!l) return undefined
  if (l === '#') return { allOf: LETTERS.map((c) => leaf('titleSort', 'doesNotBeginWith', c)) }
  return leaf('titleSort', 'beginsWith', l)
}

// the referential endpoint serves strings, but the ageRating leaf expects a number
function ageRatingValue(v: string): string | number {
  return /^\d+$/.test(v) ? Number(v) : v
}

/** a single id stays a plain leaf so the body matches the browse-page shape; multiple ids OR together */
export function libraryScopeCondition(libraryIds: string[]): SearchCondition | undefined {
  if (libraryIds.length === 0) return undefined
  if (libraryIds.length === 1) return leaf('libraryId', 'is', libraryIds[0])
  return { anyOf: libraryIds.map((id) => leaf('libraryId', 'is', id)) }
}

function finalize(state: FilterState, conditions: SearchCondition[]): { condition?: SearchCondition; fullTextSearch?: string } {
  return {
    ...(conditions.length > 0 && { condition: conditions.length === 1 ? conditions[0] : { allOf: conditions } }),
    ...(state.q.trim() && { fullTextSearch: state.q.trim() }),
  }
}

export function buildSeriesSearch(state: FilterState, libraryId?: string, extra?: SearchCondition): SeriesSearch {
  const mode = (g: GroupKey): GroupMode => (state.matchAll.includes(g) ? 'all' : 'any')
  const readOp: SearchOperator = state.exclude.includes('readStatus') ? 'isNot' : 'is'
  const conditions: SearchCondition[] = []
  // pins the result set, e.g. series/collection membership on detail pages
  if (extra) conditions.push(extra)
  if (libraryId) conditions.push(leaf('libraryId', 'is', libraryId))
  push(conditions, combine(state.readStatus.map((v) => leaf('readStatus', readOp, v)), mode('readStatus')))
  push(conditions, combine(state.seriesStatus.map((v) => leaf('seriesStatus', 'is', v)), mode('seriesStatus')))
  push(conditions, letterCondition(state.letter))
  push(conditions, combine(state.complete.map((v) => boolLeaf('complete', v)), mode('complete')))
  push(conditions, combine(state.oneshot.map((v) => boolLeaf('oneShot', v)), mode('oneshot')))
  push(conditions, combine(state.deleted.map((v) => boolLeaf('deleted', v)), mode('deleted')))
  push(conditions, combine(state.publishers.map((v) => leaf('publisher', 'is', v)), mode('publishers')))
  push(conditions, combine(state.genres.map((v) => leaf('genre', 'is', v)), mode('genres')))
  push(conditions, combine(state.tags.map((v) => leaf('tag', 'is', v)), mode('tags')))
  push(conditions, combine(state.sharingLabels.map((v) => leaf('sharingLabel', 'is', v)), mode('sharingLabels')))
  push(
    conditions,
    combine(
      state.ageRatings.map((v) => (v === AGE_RATING_UNSET ? leaf('ageRating', 'isNull') : leaf('ageRating', 'is', ageRatingValue(v)))),
      mode('ageRatings'),
    ),
  )
  push(conditions, combine(state.languages.map((v) => leaf('language', 'is', v)), mode('languages')))
  push(conditions, combine(yearConditions(state.releaseYears), mode('releaseYears')))
  push(conditions, combine(authorConditions(state.authors), mode('authors')))
  push(conditions, libraryScopeCondition(state.libraries))
  push(
    conditions,
    combine(
      state.collections.map((v) => leaf('collectionId', state.exclude.includes('collections') ? 'isNot' : 'is', v)),
      mode('collections'),
    ),
  )
  return finalize(state, conditions)
}

export function buildBookSearch(state: FilterState, libraryId?: string, extra?: SearchCondition): BookSearch {
  const mode = (g: GroupKey): GroupMode => (state.matchAll.includes(g) ? 'all' : 'any')
  const readOp: SearchOperator = state.exclude.includes('readStatus') ? 'isNot' : 'is'
  const conditions: SearchCondition[] = []
  // pins the result set, e.g. series/read-list membership on detail pages
  if (extra) conditions.push(extra)
  if (libraryId) conditions.push(leaf('libraryId', 'is', libraryId))
  push(conditions, combine(state.readStatus.map((v) => leaf('readStatus', readOp, v)), mode('readStatus')))
  push(conditions, combine(state.tags.map((v) => leaf('tag', 'is', v)), mode('tags')))
  push(conditions, combine(state.mediaProfiles.map((v) => leaf('mediaProfile', 'is', v)), mode('mediaProfiles')))
  push(conditions, combine(state.mediaStatuses.map((v) => leaf('mediaStatus', 'is', v)), mode('mediaStatuses')))
  // "no poster" must also match books with zero thumbnail rows, so it is isNot selected, not is unselected
  push(
    conditions,
    combine(
      state.poster.map((v) => (v === 'missing' ? leaf('poster', 'isNot', { selected: true }) : leaf('poster', 'is', { selected: true }))),
      mode('poster'),
    ),
  )
  push(conditions, combine(yearConditions(state.releaseYears), mode('releaseYears')))
  push(conditions, combine(authorConditions(state.authors), mode('authors')))
  push(conditions, libraryScopeCondition(state.libraries))
  push(
    conditions,
    combine(
      state.readlists.map((v) => leaf('readListId', state.exclude.includes('readlists') ? 'isNot' : 'is', v)),
      mode('readlists'),
    ),
  )
  return finalize(state, conditions)
}
