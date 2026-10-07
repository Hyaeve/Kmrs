export type GroupKey =
  | 'readStatus'
  | 'seriesStatus'
  | 'complete'
  | 'oneshot'
  | 'deleted'
  | 'letter'
  | 'publishers'
  | 'genres'
  | 'tags'
  | 'sharingLabels'
  | 'ageRatings'
  | 'languages'
  | 'releaseYears'
  | 'authors'
  | 'mediaProfiles'
  | 'mediaStatuses'
  | 'poster'
  | 'libraries'
  | 'readlists'
  | 'collections'

/** How multiple values inside one group combine in the search DSL. */
export type GroupMode = 'any' | 'all'

export interface AuthorFilter {
  name: string
  role: string
}

export interface FilterState {
  q: string
  readStatus: string[]
  seriesStatus: string[]
  complete: string[]
  oneshot: string[]
  deleted: string[]
  letter: string[]
  publishers: string[]
  genres: string[]
  tags: string[]
  sharingLabels: string[]
  ageRatings: string[]
  languages: string[]
  releaseYears: string[]
  authors: AuthorFilter[]
  mediaProfiles: string[]
  mediaStatuses: string[]
  poster: string[]
  /** entity ids (libraries / read lists / collections); only smart-list editing offers these */
  libraries: string[]
  readlists: string[]
  collections: string[]
  /** groups combined with allOf instead of the default anyOf */
  matchAll: GroupKey[]
  /** groups whose values are matched with isNot instead of is */
  exclude: GroupKey[]
}

export type ReferentialKind =
  | 'publishers'
  | 'genres'
  | 'tags'
  | 'bookTags'
  | 'sharingLabels'
  | 'ageRatings'
  | 'languages'
  | 'releaseDates'

export interface FilterGroupDef {
  key: GroupKey
  /** translation key, resolved with t() at render sites */
  labelKey: string
  kind: 'enum' | 'referential' | 'authors' | 'letters' | 'entities'
  options?: { value: string; labelKey: string }[]
  referential?: ReferentialKind
  /** which entity list feeds an 'entities' group */
  entities?: 'libraries' | 'collections' | 'readlists'
  /** hidden from non-admin users */
  adminOnly?: boolean
  /** offers an is / is-not switch once values are selected */
  negatable?: boolean
  /** yes/no enum rendered as one tri-state row instead of a chip group */
  flag?: boolean
}

export interface SortOption {
  /** translation key, resolved with t() at render sites */
  labelKey: string
  property: string
}

export interface SortState {
  property: string
  direction: 'asc' | 'desc'
}

const READ_STATUS_OPTIONS = [
  { value: 'READ', labelKey: 'filters:option.readStatus.read' },
  { value: 'UNREAD', labelKey: 'filters:option.readStatus.unread' },
  { value: 'IN_PROGRESS', labelKey: 'filters:option.readStatus.inProgress' },
]

const YES_NO_OPTIONS = [
  { value: 'true', labelKey: 'filters:option.yesNo.yes' },
  { value: 'false', labelKey: 'filters:option.yesNo.no' },
]

/** First-letter navigation values; '#' stands for titles not starting with a letter. */
export const LETTERS = [...'ABCDEFGHIJKLMNOPQRSTUVWXYZ']

// v1 served unset age ratings as the string "None" while the v2 endpoint drops NULLs, so the
// filter UI injects the sentinel itself; the value stays "None" so v1-era bookmarked URLs keep working
export const AGE_RATING_UNSET = 'None'

export const SERIES_FILTER_GROUPS: FilterGroupDef[] = [
  { key: 'readStatus', labelKey: 'filters:group.readStatus', kind: 'enum', options: READ_STATUS_OPTIONS, negatable: true },
  {
    key: 'seriesStatus',
    labelKey: 'filters:group.seriesStatus',
    kind: 'enum',
    options: [
      { value: 'ONGOING', labelKey: 'common:seriesStatus.ongoing' },
      { value: 'ENDED', labelKey: 'common:seriesStatus.ended' },
      { value: 'ABANDONED', labelKey: 'common:seriesStatus.abandoned' },
      { value: 'HIATUS', labelKey: 'common:seriesStatus.hiatus' },
    ],
  },
  { key: 'letter', labelKey: 'filters:group.letter', kind: 'letters' },
  { key: 'complete', labelKey: 'filters:group.complete', kind: 'enum', options: YES_NO_OPTIONS, flag: true },
  { key: 'oneshot', labelKey: 'common:oneshot', kind: 'enum', options: YES_NO_OPTIONS, flag: true },
  { key: 'deleted', labelKey: 'filters:group.deleted', kind: 'enum', options: YES_NO_OPTIONS, adminOnly: true, flag: true },
  { key: 'publishers', labelKey: 'filters:group.publishers', kind: 'referential', referential: 'publishers' },
  { key: 'genres', labelKey: 'filters:group.genres', kind: 'referential', referential: 'genres' },
  // series-level tag options come from the combined series+aggregation tag view, so oneshots (whose
  // tags live on the book) contribute too; the series search `tag` condition matches the same union
  { key: 'tags', labelKey: 'filters:group.tags', kind: 'referential', referential: 'tags' },
  { key: 'sharingLabels', labelKey: 'filters:group.sharingLabels', kind: 'referential', referential: 'sharingLabels' },
  { key: 'ageRatings', labelKey: 'filters:group.ageRatings', kind: 'referential', referential: 'ageRatings' },
  { key: 'languages', labelKey: 'filters:group.languages', kind: 'referential', referential: 'languages' },
  { key: 'releaseYears', labelKey: 'filters:group.releaseYears', kind: 'referential', referential: 'releaseDates' },
  { key: 'authors', labelKey: 'filters:group.authors', kind: 'authors' },
]

export const BOOK_FILTER_GROUPS: FilterGroupDef[] = [
  { key: 'readStatus', labelKey: 'filters:group.readStatus', kind: 'enum', options: READ_STATUS_OPTIONS, negatable: true },
  { key: 'tags', labelKey: 'filters:group.tags', kind: 'referential', referential: 'bookTags' },
  {
    key: 'mediaProfiles',
    labelKey: 'filters:group.mediaProfiles',
    kind: 'enum',
    options: [
      { value: 'DIVINA', labelKey: 'filters:option.mediaProfile.divina' },
      { value: 'PDF', labelKey: 'filters:option.mediaProfile.pdf' },
      { value: 'EPUB', labelKey: 'filters:option.mediaProfile.epub' },
    ],
  },
  {
    key: 'mediaStatuses',
    labelKey: 'filters:group.mediaStatuses',
    kind: 'enum',
    options: [
      { value: 'READY', labelKey: 'filters:option.mediaStatus.ready' },
      { value: 'ERROR', labelKey: 'common:cardStatus.error' },
      { value: 'UNKNOWN', labelKey: 'common:state.unknown' },
      { value: 'UNSUPPORTED', labelKey: 'common:cardStatus.unsupported' },
      { value: 'OUTDATED', labelKey: 'filters:option.mediaStatus.outdated' },
    ],
  },
  {
    key: 'poster',
    labelKey: 'filters:group.poster',
    kind: 'enum',
    options: [
      { value: 'selected', labelKey: 'filters:option.poster.selected' },
      { value: 'missing', labelKey: 'filters:option.poster.missing' },
    ],
  },
  { key: 'releaseYears', labelKey: 'filters:group.releaseYears', kind: 'referential', referential: 'releaseDates' },
  { key: 'authors', labelKey: 'filters:group.authors', kind: 'authors' },
]

export const SERIES_SORT_OPTIONS: SortOption[] = [
  { labelKey: 'filters:sort.title', property: 'metadata.titleSort' },
  { labelKey: 'filters:sort.dateAdded', property: 'createdDate' },
  { labelKey: 'filters:sort.dateUpdated', property: 'lastModifiedDate' },
  { labelKey: 'filters:sort.releaseDate', property: 'booksMetadata.releaseDate' },
  { labelKey: 'filters:sort.lastRead', property: 'readDate' },
  { labelKey: 'filters:sort.folderName', property: 'name' },
  { labelKey: 'filters:sort.booksCount', property: 'booksCount' },
  { labelKey: 'filters:sort.random', property: 'random' },
]

export const BOOK_SORT_OPTIONS: SortOption[] = [
  { labelKey: 'filters:sort.title', property: 'metadata.titleSort' },
  { labelKey: 'filters:sort.series', property: 'series' },
  { labelKey: 'filters:sort.number', property: 'metadata.numberSort' },
  { labelKey: 'filters:sort.dateAdded', property: 'createdDate' },
  { labelKey: 'filters:sort.dateUpdated', property: 'lastModifiedDate' },
  { labelKey: 'filters:sort.releaseDate', property: 'metadata.releaseDate' },
  { labelKey: 'filters:sort.lastRead', property: 'readProgress.readDate' },
  { labelKey: 'filters:sort.fileName', property: 'url' },
  { labelKey: 'filters:sort.fileSize', property: 'fileSize' },
  { labelKey: 'filters:sort.pages', property: 'media.pagesCount' },
]

/** book filter groups for single-context pages (series detail, read list): release years are
    series-level metadata, meaningless for one series and unavailable for a read list */
export const BOOK_DETAIL_FILTER_GROUPS: FilterGroupDef[] = BOOK_FILTER_GROUPS.filter((g) => g.key !== 'releaseYears')

// smart-list editing scopes by whole entities, so libraries and membership get their own groups
export const SMARTLIST_BOOK_FILTER_GROUPS: FilterGroupDef[] = [
  { key: 'libraries', labelKey: 'filters:group.libraries', kind: 'entities', entities: 'libraries' },
  { key: 'readlists', labelKey: 'filters:group.readlists', kind: 'entities', entities: 'readlists', negatable: true },
  ...BOOK_FILTER_GROUPS,
]

export const SMARTLIST_SERIES_FILTER_GROUPS: FilterGroupDef[] = [
  { key: 'libraries', labelKey: 'filters:group.libraries', kind: 'entities', entities: 'libraries' },
  { key: 'collections', labelKey: 'filters:group.collections', kind: 'entities', entities: 'collections', negatable: true },
  ...SERIES_FILTER_GROUPS,
]

/** sorting by series is meaningless inside a single series' book list */
export const SERIES_BOOK_SORT_OPTIONS = BOOK_SORT_OPTIONS.filter((o) => o.property !== 'series')

// the list-order properties only exist when the search condition joins the list (readListId/collectionId leaf)
export const READLIST_BOOK_SORT_OPTIONS: SortOption[] = [
  { labelKey: 'filters:sort.listOrder', property: 'readList.number' },
  ...SERIES_BOOK_SORT_OPTIONS,
]

export const COLLECTION_SERIES_SORT_OPTIONS: SortOption[] = [
  { labelKey: 'filters:sort.listOrder', property: 'collection.number' },
  ...SERIES_SORT_OPTIONS,
]

export const SERIES_DEFAULT_SORT: SortState = { property: 'metadata.titleSort', direction: 'asc' }
export const BOOK_DEFAULT_SORT: SortState = { property: 'name', direction: 'asc' }
export const SERIES_BOOK_DEFAULT_SORT: SortState = { property: 'metadata.numberSort', direction: 'asc' }
export const READLIST_ORDER_SORT: SortState = { property: 'readList.number', direction: 'asc' }
/** kmrs orders unordered read lists by release date */
export const READLIST_DATE_SORT: SortState = { property: 'metadata.releaseDate', direction: 'asc' }
export const COLLECTION_ORDER_SORT: SortState = { property: 'collection.number', direction: 'asc' }
