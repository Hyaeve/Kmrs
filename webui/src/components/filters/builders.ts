import type { BookSearch, ConditionLeaf, SearchCondition, SearchOperator, SeriesSearch } from '@/lib/api/types'
import { AGE_RATING_UNSET, LETTERS, NOTABLE_ENTITY_ANYKIND, REFERENTIAL_ANY, type AuthorFilter, type FilterState, type GroupKey, type GroupMode, type NotableEntityFilter } from './types'

function leaf(key: string, operator: SearchOperator, value?: unknown): ConditionLeaf {
  return value === undefined ? { [key]: { operator } } : { [key]: { operator, value } }
}

// unset is NULL for genre/tag/sharingLabel but the empty string for publisher/language
function referentialLeaf(key: string, unset: 'null' | 'empty', op: 'is' | 'isNot', v: string): ConditionLeaf {
  if (v !== REFERENTIAL_ANY) return leaf(key, op, v)
  if (unset === 'empty') return leaf(key, op === 'isNot' ? 'is' : 'isNot', '')
  return leaf(key, op === 'isNot' ? 'isNull' : 'isNotNull')
}

function combine(conditions: SearchCondition[], mode: GroupMode): SearchCondition | undefined {
  if (conditions.length === 0) return undefined
  if (conditions.length === 1) return conditions[0]
  return mode === 'any' ? { anyOf: conditions } : { allOf: conditions }
}

function push(out: SearchCondition[], condition: SearchCondition | undefined) {
  if (condition) out.push(condition)
}

function yearConditions(years: string[], negated: boolean): SearchCondition[] {
  return years.map((y) => {
    if (y === REFERENTIAL_ANY) return leaf('releaseDate', negated ? 'isNull' : 'isNotNull')
    const year = Number(y)
    // date ops compare strict against the UTC date, so the bounds sit just outside the year
    if (!negated)
      return {
        allOf: [
          { releaseDate: { operator: 'after', dateTime: `${year - 1}-12-31T00:00:00Z` } },
          { releaseDate: { operator: 'before', dateTime: `${year + 1}-01-01T00:00:00Z` } },
        ],
      }
    // a year's complement covers unknown dates too
    return {
      anyOf: [
        { releaseDate: { operator: 'after', dateTime: `${year}-12-31T00:00:00Z` } },
        { releaseDate: { operator: 'before', dateTime: `${year}-01-01T00:00:00Z` } },
        leaf('releaseDate', 'isNull'),
      ],
    }
  })
}

function authorConditions(authors: AuthorFilter[]): SearchCondition[] {
  return authors.flatMap((a) => {
    // a nameless entry is the role-level any/none pseudo; an entirely empty one would match
    // anything that has an author, so it is rejected at parse time and dropped here
    if (!a.name && !a.role) return []
    const value = a.name ? (a.role ? { name: a.name, role: a.role } : { name: a.name }) : { role: a.role }
    return [leaf('author', a.exclude ? 'isNot' : 'is', value)]
  })
}

function notableEntityConditions(entities: NotableEntityFilter[]): SearchCondition[] {
  return entities.map((e) =>
    leaf('notableEntity', e.exclude ? 'isNot' : 'is', {
      // the any-kind panel omits kind to match across kinds; a nameless entry is the
      // any/none match of its kind
      ...(e.kind === NOTABLE_ENTITY_ANYKIND ? {} : { kind: e.kind }),
      ...(e.name ? { name: e.name } : {}),
    }),
  )
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
  const isOp = (g: GroupKey): 'is' | 'isNot' => (state.exclude.includes(g) ? 'isNot' : 'is')
  const conditions: SearchCondition[] = []
  // pins the result set, e.g. series/collection membership on detail pages
  if (extra) conditions.push(extra)
  if (libraryId) conditions.push(leaf('libraryId', 'is', libraryId))
  push(conditions, combine(state.readStatus.map((v) => leaf('readStatus', isOp('readStatus'), v)), mode('readStatus')))
  push(conditions, combine(state.seriesStatus.map((v) => leaf('seriesStatus', isOp('seriesStatus'), v)), mode('seriesStatus')))
  push(conditions, letterCondition(state.letter))
  push(conditions, combine(state.complete.map((v) => boolLeaf('complete', v)), mode('complete')))
  push(conditions, combine(state.oneshot.map((v) => boolLeaf('oneShot', v)), mode('oneshot')))
  push(conditions, combine(state.deleted.map((v) => boolLeaf('deleted', v)), mode('deleted')))
  push(conditions, combine(state.publishers.map((v) => referentialLeaf('publisher', 'empty', isOp('publishers'), v)), mode('publishers')))
  push(conditions, combine(state.genres.map((v) => referentialLeaf('genre', 'null', isOp('genres'), v)), mode('genres')))
  push(conditions, combine(state.tags.map((v) => referentialLeaf('tag', 'null', isOp('tags'), v)), mode('tags')))
  push(conditions, combine(state.sharingLabels.map((v) => referentialLeaf('sharingLabel', 'null', isOp('sharingLabels'), v)), mode('sharingLabels')))
  push(
    conditions,
    combine(
      state.ageRatings.map((v) =>
        v === AGE_RATING_UNSET
          ? leaf('ageRating', state.exclude.includes('ageRatings') ? 'isNotNull' : 'isNull')
          : leaf('ageRating', isOp('ageRatings'), ageRatingValue(v)),
      ),
      mode('ageRatings'),
    ),
  )
  push(conditions, combine(state.languages.map((v) => referentialLeaf('language', 'empty', isOp('languages'), v)), mode('languages')))
  push(conditions, combine(yearConditions(state.releaseYears, state.exclude.includes('releaseYears')), mode('releaseYears')))
  push(conditions, combine(authorConditions(state.authors), mode('authors')))
  push(conditions, combine(notableEntityConditions(state.notableEntities), mode('notableEntities')))
  push(conditions, libraryScopeCondition(state.libraries))
  push(
    conditions,
    combine(
      state.collections.map((v) => leaf('collectionId', isOp('collections'), v)),
      mode('collections'),
    ),
  )
  return finalize(state, conditions)
}

export function buildBookSearch(state: FilterState, libraryId?: string, extra?: SearchCondition): BookSearch {
  const mode = (g: GroupKey): GroupMode => (state.matchAll.includes(g) ? 'all' : 'any')
  const isOp = (g: GroupKey): 'is' | 'isNot' => (state.exclude.includes(g) ? 'isNot' : 'is')
  const conditions: SearchCondition[] = []
  // pins the result set, e.g. series/read-list membership on detail pages
  if (extra) conditions.push(extra)
  if (libraryId) conditions.push(leaf('libraryId', 'is', libraryId))
  push(conditions, combine(state.readStatus.map((v) => leaf('readStatus', isOp('readStatus'), v)), mode('readStatus')))
  push(conditions, combine(state.tags.map((v) => referentialLeaf('tag', 'null', isOp('tags'), v)), mode('tags')))
  push(conditions, combine(state.mediaProfiles.map((v) => leaf('mediaProfile', isOp('mediaProfiles'), v)), mode('mediaProfiles')))
  push(conditions, combine(state.mediaStatuses.map((v) => leaf('mediaStatus', isOp('mediaStatuses'), v)), mode('mediaStatuses')))
  // "no poster" must also match books with zero thumbnail rows, so it is isNot selected, not is unselected
  push(
    conditions,
    combine(
      state.poster.map((v) =>
        v === 'missing'
          ? leaf('poster', 'isNot', { selected: true })
          : v === 'selected'
            ? leaf('poster', 'is', { selected: true })
            : leaf('poster', 'is', { type: v }),
      ),
      mode('poster'),
    ),
  )
  push(conditions, combine(yearConditions(state.releaseYears, state.exclude.includes('releaseYears')), mode('releaseYears')))
  push(conditions, combine(authorConditions(state.authors), mode('authors')))
  push(conditions, combine(notableEntityConditions(state.notableEntities), mode('notableEntities')))
  push(conditions, libraryScopeCondition(state.libraries))
  push(
    conditions,
    combine(
      state.readlists.map((v) => leaf('readListId', isOp('readlists'), v)),
      mode('readlists'),
    ),
  )
  return finalize(state, conditions)
}
