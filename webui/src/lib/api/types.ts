// DTOs mirroring the Komga 1.27 OpenAPI schemas (camelCase field names as served).

export interface Page<T> {
  content: T[]
  pageable: {
    pageNumber: number
    pageSize: number
    sort: { empty: boolean; sorted: boolean; unsorted: boolean }
    offset: number
    paged: boolean
    unpaged: boolean
  }
  totalElements: number
  totalPages: number
  last: boolean
  first: boolean
  number: number
  size: number
  numberOfElements: number
  sort: { empty: boolean; sorted: boolean; unsorted: boolean }
  empty: boolean
}

export interface PageParams {
  page?: number
  size?: number
  sort?: string[]
  unpaged?: boolean
}

export type ReadingDirection = 'LEFT_TO_RIGHT' | 'RIGHT_TO_LEFT' | 'VERTICAL' | 'WEBTOON'
export type SeriesStatus = 'ONGOING' | 'ENDED' | 'ABANDONED' | 'HIATUS'
export type ReadStatus = 'READ' | 'UNREAD' | 'IN_PROGRESS'
export type MediaStatus = 'UNKNOWN' | 'ERROR' | 'READY' | 'UNSUPPORTED' | 'OUTDATED'
export type MediaProfile = 'DIVINA' | 'PDF' | 'EPUB'
export type Role = 'ADMIN' | 'USER' | 'FILE_DOWNLOAD' | 'PAGE_STREAMING' | 'KOBO_SYNC' | 'KOREADER_SYNC'

export type ScanInterval = 'DISABLED' | 'HOURLY' | 'EVERY_6H' | 'EVERY_12H' | 'DAILY' | 'WEEKLY'
export type SeriesCover = 'FIRST' | 'FIRST_UNREAD_OR_FIRST' | 'FIRST_UNREAD_OR_LAST' | 'LAST'

export interface LibraryDto {
  id: string
  name: string
  /** Empty string for non-admin users. */
  root: string
  unavailable: boolean
  importComicInfoBook: boolean
  importComicInfoSeries: boolean
  importComicInfoCollection: boolean
  importComicInfoReadList: boolean
  importComicInfoSeriesAppendVolume: boolean
  importEpubBook: boolean
  importEpubSeries: boolean
  importMylarSeries: boolean
  importLocalArtwork: boolean
  importBarcodeIsbn: boolean
  scanForceModifiedTime: boolean
  scanInterval: ScanInterval
  scanOnStartup: boolean
  scanCbx: boolean
  scanPdf: boolean
  scanEpub: boolean
  scanDirectoryExclusions: string[]
  repairExtensions: boolean
  convertToCbz: boolean
  emptyTrashAfterScan: boolean
  seriesCover: SeriesCover
  hashFiles: boolean
  hashPages: boolean
  hashKoreader: boolean
  analyzeDimensions: boolean
  oneshotsDirectory?: string | null
}

/** All fields optional except name/root; omitted fields fall back to server defaults. */
export interface LibraryCreationDto {
  name: string
  root: string
  importComicInfoBook?: boolean
  importComicInfoSeries?: boolean
  importComicInfoCollection?: boolean
  importComicInfoReadList?: boolean
  importComicInfoSeriesAppendVolume?: boolean
  importEpubBook?: boolean
  importEpubSeries?: boolean
  importMylarSeries?: boolean
  importLocalArtwork?: boolean
  importBarcodeIsbn?: boolean
  scanForceModifiedTime?: boolean
  scanInterval?: ScanInterval
  scanOnStartup?: boolean
  scanCbx?: boolean
  scanPdf?: boolean
  scanEpub?: boolean
  scanDirectoryExclusions?: string[]
  repairExtensions?: boolean
  convertToCbz?: boolean
  emptyTrashAfterScan?: boolean
  seriesCover?: SeriesCover
  hashFiles?: boolean
  hashPages?: boolean
  hashKoreader?: boolean
  analyzeDimensions?: boolean
  oneshotsDirectory?: string
}

/** Patch semantics: omitted fields keep their current value; null clears nullable fields. */
export type LibraryUpdateDto = Partial<Omit<LibraryCreationDto, 'oneshotsDirectory' | 'name' | 'root'>> & {
  name?: string
  root?: string
  oneshotsDirectory?: string | null
}

export interface AlternateTitleDto {
  label: string
  title: string
}

export interface WebLinkDto {
  label: string
  url: string
}

export interface AuthorDto {
  name: string
  role: string
}

export interface SeriesMetadataDto {
  title: string
  titleLock: boolean
  titleSort: string
  titleSortLock: boolean
  status: SeriesStatus
  statusLock: boolean
  summary: string
  summaryLock: boolean
  publisher: string
  publisherLock: boolean
  ageRating?: number
  ageRatingLock: boolean
  language: string
  languageLock: boolean
  readingDirection: ReadingDirection
  readingDirectionLock: boolean
  genres: string[]
  genresLock: boolean
  tags: string[]
  tagsLock: boolean
  sharingLabels: string[]
  sharingLabelsLock: boolean
  alternateTitles: AlternateTitleDto[]
  alternateTitlesLock: boolean
  links: WebLinkDto[]
  linksLock: boolean
  totalBookCount?: number
  totalBookCountLock: boolean
  created: string
  lastModified: string
}

export interface BookMetadataAggregationDto {
  authors: AuthorDto[]
  tags: string[]
  releaseDate?: string
  summary: string
  summaryNumber: string
  created: string
  lastModified: string
}

export interface SeriesDto {
  id: string
  libraryId: string
  name: string
  url: string
  created: string
  lastModified: string
  fileLastModified: string
  deleted: boolean
  oneshot: boolean
  booksCount: number
  booksReadCount: number
  booksUnreadCount: number
  booksInProgressCount: number
  metadata: SeriesMetadataDto
  booksMetadata: BookMetadataAggregationDto
}

export interface MediaDto {
  status: MediaStatus
  mediaType: string
  mediaProfile: MediaProfile
  pagesCount: number
  comment: string
  epubDivinaCompatible: boolean
  epubIsKepub: boolean
}

export interface BookMetadataDto {
  title: string
  titleLock: boolean
  summary: string
  summaryLock: boolean
  number: string
  numberLock: boolean
  numberSort: number
  numberSortLock: boolean
  releaseDate?: string
  releaseDateLock: boolean
  authors: AuthorDto[]
  authorsLock: boolean
  tags: string[]
  tagsLock: boolean
  isbn: string
  isbnLock: boolean
  links: WebLinkDto[]
  linksLock: boolean
  created: string
  lastModified: string
}

export interface ReadProgressDto {
  page: number
  completed: boolean
  readDate?: string
  created: string
  lastModified: string
  deviceId: string
  deviceName: string
}

export interface BookDto {
  id: string
  seriesId: string
  seriesTitle: string
  libraryId: string
  name: string
  url: string
  number: number
  created: string
  lastModified: string
  fileLastModified: string
  deleted: boolean
  oneshot: boolean
  size: string
  sizeBytes: number
  fileHash: string
  media: MediaDto
  metadata: BookMetadataDto
  readProgress?: ReadProgressDto
}

export interface PageDto {
  number: number
  fileName: string
  mediaType: string
  width?: number
  height?: number
  size: string
  sizeBytes?: number
}

export interface CollectionDto {
  id: string
  name: string
  ordered: boolean
  filtered: boolean
  seriesIds: string[]
  createdDate: string
  lastModifiedDate: string
}

export interface ReadListDto {
  id: string
  name: string
  summary: string
  ordered: boolean
  filtered: boolean
  bookIds: string[]
  createdDate: string
  lastModifiedDate: string
}

export interface UserDto {
  id: string
  email: string
  roles: Role[]
  sharedAllLibraries: boolean
  sharedLibrariesIds: string[]
  labelsAllow: string[]
  labelsExclude: string[]
  ageRestriction?: { age: number; restriction: 'ALLOW_ONLY' | 'EXCLUDE' }
}

export interface GroupCountDto {
  group: string
  count: number
}

export interface OAuth2ClientDto {
  registrationId: string
  name: string
}

export interface ClaimStatus {
  isClaimed: boolean
}

export interface ApiKeyDto {
  id: string
  key?: string
  comment: string
  userId: string
  createdDate: string
  lastModifiedDate: string
}

export interface AuthenticationActivityDto {
  dateTime: string
  email?: string
  userId?: string
  apiKeyId?: string
  apiKeyComment?: string
  ip?: string
  userAgent?: string
  source?: string
  error?: string
  success: boolean
}

export interface ActuatorInfo {
  git?: { branch?: string; commit?: { id?: string; time?: string } }
  build?: { artifact?: string; name?: string; version?: string; group?: string }
}

// ---- Admin: user management ----

export interface SharedLibrariesDto {
  all: boolean
  libraryIds?: string[]
}

export interface AgeRestrictionDto {
  age: number
  restriction: 'ALLOW_ONLY' | 'EXCLUDE' | 'NONE'
}

export interface UserCreationDto {
  email: string
  password: string
  roles?: Role[]
  ageRestriction?: AgeRestrictionDto
  labelsAllow?: string[]
  labelsExclude?: string[]
  sharedLibraries?: SharedLibrariesDto
}

/** Patch semantics: omitting a field keeps its current value. */
export interface UserUpdateDto {
  roles?: Role[]
  sharedLibraries?: SharedLibrariesDto
  ageRestriction?: AgeRestrictionDto
  labelsAllow?: string[]
  labelsExclude?: string[]
}

// ---- Admin: server settings ----

export type ThumbnailSize = 'DEFAULT' | 'MEDIUM' | 'LARGE' | 'XLARGE'

/** A setting resolvable from config file, database override, or default. */
export interface SettingMultiSource<T> {
  configurationSource: T | null
  databaseSource: T | null
  effectiveValue: T | null
}

export interface SettingsDto {
  deleteEmptyCollections?: boolean
  deleteEmptyReadLists?: boolean
  rememberMeDurationDays?: number
  thumbnailSize?: ThumbnailSize
  taskPoolSize?: number
  serverPort?: SettingMultiSource<number>
  serverContextPath?: SettingMultiSource<string>
  koboProxy?: boolean
  koboPort?: number
  kepubifyPath?: SettingMultiSource<string>
  maxUploadFileSizeBytes?: number
}

/** Patch semantics: omit = unchanged; null on multi-source fields clears the database override. */
export interface SettingsUpdateDto {
  deleteEmptyCollections?: boolean
  deleteEmptyReadLists?: boolean
  rememberMeDurationDays?: number
  renewRememberMeKey?: boolean
  thumbnailSize?: ThumbnailSize
  taskPoolSize?: number
  serverPort?: number | null
  serverContextPath?: string | null
  koboProxy?: boolean
  koboPort?: number | null
  kepubifyPath?: string | null
}

// ---- Admin: filesystem browser ----

export interface PathDto {
  type: 'directory' | 'file'
  name: string
  path: string
}

export interface DirectoryListingDto {
  parent?: string
  directories: PathDto[]
  files: PathDto[]
}

// ---- Admin: actuator ----

export interface ActuatorHealth {
  status: string
  components?: {
    db?: { status: string; details?: Record<string, unknown> }
    diskSpace?: {
      status: string
      details?: { total?: number; free?: number; threshold?: number; exists?: boolean }
    }
  }
}

export interface ScheduledTaskDto {
  runnable: { target: string }
  initialDelay: number
  interval: number
}

export interface ScheduledTasksDto {
  cron: unknown[]
  fixedDelay: unknown[]
  fixedRate: ScheduledTaskDto[]
  custom: unknown[]
}

export interface SessionDto {
  id: string
  attributeNames: string[]
  creationTime: string
  lastAccessedTime: string
  maxInactiveInterval: number
  expired: boolean
}

export interface TaskQueueStatus {
  count: number
  countByType: Record<string, number>
}

// ---- Search DSL (POST /list bodies) ----

export type SearchOperator =
  | 'is'
  | 'isNot'
  | 'isNull'
  | 'isNotNull'
  | 'isTrue'
  | 'isFalse'
  | 'contains'
  | 'beginsWith'
  | 'endsWith'
  | 'doesNotContain'
  | 'doesNotBeginWith'
  | 'doesNotEndWith'
  | 'greaterThan'
  | 'lessThan'
  | 'before'
  | 'after'
  | 'isInTheLast'
  | 'isNotInTheLast'

export type ConditionLeaf = Record<string, { operator: SearchOperator; value?: unknown; dateTime?: string }>

export interface ConditionGroup {
  allOf?: SearchCondition[]
  anyOf?: SearchCondition[]
}

export type SearchCondition = ConditionLeaf | ConditionGroup

export interface SeriesSearch {
  condition?: SearchCondition
  fullTextSearch?: string
}

export interface BookSearch {
  condition?: SearchCondition
  fullTextSearch?: string
}

// ---- kmrs-private smart lists (not part of the Komga API surface) ----

export type SmartListTarget = 'BOOK' | 'SERIES'

export type SmartListVisibility = 'PRIVATE' | 'PUBLIC' | 'SHARED'

export interface SmartListDto {
  id: string
  name: string
  summary: string
  ownerId: string
  target: SmartListTarget
  visibility: SmartListVisibility
  sharedWithUserIds: string[]
  search: BookSearch | SeriesSearch
  createdDate: string
  lastModifiedDate: string
}

export interface SmartListCreationDto {
  name: string
  summary?: string
  target: SmartListTarget
  visibility?: SmartListVisibility
  sharedWithUserIds?: string[]
  search: BookSearch | SeriesSearch
}

export interface SmartListUpdateDto {
  name?: string
  summary?: string
  target?: SmartListTarget
  visibility?: SmartListVisibility
  sharedWithUserIds?: string[]
  search?: BookSearch | SeriesSearch
}

/** minimal user directory for choosing share targets */
export interface ShareTargetDto {
  id: string
  email: string
}

// kmrs-private, mirrors ThumbnailReadListDto
export interface ThumbnailSmartListDto {
  id: string
  smartListId: string
  type: ThumbnailType
  selected: boolean
  mediaType: string
  fileSize: number
  width: number
  height: number
}

// ---- Metadata updates (PATCH bodies) ----

export interface WebLinkUpdateDto {
  label?: string
  url?: string
}

export interface AlternateTitleUpdateDto {
  label?: string
  title?: string
}

export interface AuthorUpdateDto {
  name?: string
  role?: string
}

/**
 * Patch semantics: omitted fields keep their current value. Collection and nullable fields are
 * isSet-tracked: sending null clears them, while explicit null on plain fields is a no-op.
 */
export interface SeriesMetadataUpdateDto {
  status?: SeriesStatus
  statusLock?: boolean
  title?: string
  titleLock?: boolean
  titleSort?: string
  titleSortLock?: boolean
  summary?: string
  summaryLock?: boolean
  publisher?: string
  publisherLock?: boolean
  readingDirection?: ReadingDirection | null
  readingDirectionLock?: boolean
  ageRating?: number | null
  ageRatingLock?: boolean
  language?: string
  languageLock?: boolean
  genres?: string[] | null
  genresLock?: boolean
  tags?: string[] | null
  tagsLock?: boolean
  totalBookCount?: number | null
  totalBookCountLock?: boolean
  sharingLabels?: string[] | null
  sharingLabelsLock?: boolean
  links?: WebLinkUpdateDto[] | null
  linksLock?: boolean
  alternateTitles?: AlternateTitleUpdateDto[] | null
  alternateTitlesLock?: boolean
}

/** Same patch semantics as SeriesMetadataUpdateDto. */
export interface BookMetadataUpdateDto {
  title?: string
  titleLock?: boolean
  summary?: string | null
  summaryLock?: boolean
  number?: string
  numberLock?: boolean
  numberSort?: number
  numberSortLock?: boolean
  releaseDate?: string | null
  releaseDateLock?: boolean
  authors?: AuthorUpdateDto[] | null
  authorsLock?: boolean
  tags?: string[] | null
  tagsLock?: boolean
  isbn?: string | null
  isbnLock?: boolean
  links?: WebLinkUpdateDto[] | null
  linksLock?: boolean
}

// ---- Thumbnails ----

export type ThumbnailType = 'GENERATED' | 'SIDECAR' | 'USER_UPLOADED'

export interface ThumbnailSeriesDto {
  id: string
  seriesId: string
  type: ThumbnailType
  selected: boolean
  mediaType: string
  fileSize: number
  width: number
  height: number
}

export interface ThumbnailBookDto {
  id: string
  bookId: string
  type: ThumbnailType
  selected: boolean
  mediaType: string
  fileSize: number
  width: number
  height: number
}

export interface ThumbnailSeriesCollectionDto {
  id: string
  collectionId: string
  type: ThumbnailType
  selected: boolean
  mediaType: string
  fileSize: number
  width: number
  height: number
}

export interface ThumbnailReadListDto {
  id: string
  readListId: string
  type: ThumbnailType
  selected: boolean
  mediaType: string
  fileSize: number
  width: number
  height: number
}

// ---- Admin: page hashes (duplicate pages) ----

export type PageHashAction = 'DELETE_AUTO' | 'DELETE_MANUAL' | 'IGNORE'

export interface PageHashKnownDto {
  hash: string
  size: number | null
  action: PageHashAction
  deleteCount: number
  matchCount: number
  created: string
  lastModified: string
}

export interface PageHashUnknownDto {
  hash: string
  size: number | null
  matchCount: number
}

export interface PageHashMatchDto {
  bookId: string
  url: string
  pageNumber: number
  fileName: string
  fileSize: number
  mediaType: string
}

export interface PageHashCreationDto {
  hash: string
  size?: number | null
  action: PageHashAction
}

// ---- Admin: history ----

export type HistoricalEventType =
  | 'BookFileDeleted'
  | 'SeriesFolderDeleted'
  | 'BookConverted'
  | 'BookImported'
  | 'DuplicatePageDeleted'
  | 'BookTrashed'
  | 'SeriesTrashed'
  | 'BookPurged'
  | 'SeriesPurged'

export interface HistoricalEventDto {
  id: string
  type: HistoricalEventType
  timestamp: string
  bookId: string | null
  seriesId: string | null
  properties: Record<string, string>
}

// ---- Admin: transient books ----

export interface TransientBookDto {
  id: string
  name: string
  url: string
  fileLastModified: string
  sizeBytes: number
  size: string
  status: string
  mediaType: string
  pages: PageDto[]
  files: string[]
  comment: string
  number: number | null
  seriesId: string | null
}

// ---- Client settings ----

export interface ClientSettingDto {
  value: string
  allowUnauthorized?: boolean
}

export interface ClientSettingGlobalUpdateDto {
  value: string
  allowUnauthorized: boolean
}

export interface ClientSettingUserUpdateDto {
  value: string
}

// ---- Admin: releases ----

export interface ReleaseDto {
  version: string
  releaseDate: string
  url: string
  latest: boolean
  preRelease: boolean
  description: string
}

// ---- Admin: book import ----

export type CopyMode = 'MOVE' | 'COPY' | 'HARDLINK'

export interface BookImportDto {
  sourceFile: string
  seriesId: string
  upgradeBookId?: string
  destinationName?: string
}

export interface BookImportBatchDto {
  books: BookImportDto[]
  copyMode: CopyMode
}

// ---- Admin: readlist ComicRack matching ----

export interface ReadListRequestMatchDto {
  readListMatch: ReadListMatchDto
  requests: ReadListRequestBookMatchesDto[]
  errorCode: string
}

export interface ReadListMatchDto {
  name: string
  errorCode: string
}

export interface ReadListRequestBookMatchesDto {
  request: ReadListRequestBookDto
  matches: ReadListRequestBookMatchDto[]
}

export interface ReadListRequestBookDto {
  series: string[]
  number: string
}

export interface ReadListRequestBookMatchDto {
  series: ReadListRequestBookMatchSeriesDto
  books: ReadListRequestBookMatchBookDto[]
}

export interface ReadListRequestBookMatchSeriesDto {
  seriesId: string
  title: string
  releaseDate: string | null
}

export interface ReadListRequestBookMatchBookDto {
  bookId: string
  number: string
  title: string
}

// ---- Komf integration ----

export type KomfIntegrationState = 'pending' | 'connected' | 'error'

export interface KomfIntegrationDto {
  configured: boolean
  /** unset when unconfigured and no preset exists in the kmrs config */
  url?: string
  baseUrl?: string
  /** whether a per-integration auth key is stored; the value itself is write-only */
  authKeySet: boolean
  /** absent until the integration is configured */
  state?: KomfIntegrationState
  lastError?: string
  komfReachable: boolean
}

export interface KomfIntegrationUpdateDto {
  url?: string
  baseUrl?: string
  /** blank clears the stored override; omit the field to keep it */
  authKey?: string
}

export interface KomfSeriesSearchResult {
  url?: string
  imageUrl?: string
  title: string
  provider: string
  resultId: string
  mediaType?: string
  language?: string
}

export interface KomfIdentifyRequest {
  libraryId?: string
  seriesId: string
  provider: string
  providerSeriesId: string
}

export interface KomfMetadataJobResponse {
  id: string
}

export type KomfOAuthProvider = 'anilist' | 'mal' | 'bangumi' | 'mangabaka'

// komf's own schema, passed through verbatim
export interface KomfOAuthStatus {
  logged_in: boolean
  username: string | null
}

// ---- Per-user tracker sync (komf-backed) ----

export type TrackerTrackMode = 'auto' | 'chapter' | 'volume'

export interface TrackerLink {
  seriesId: string
  provider: KomfOAuthProvider
  trackId: string
  title?: string
  trackMode: TrackerTrackMode
  chapterOffset: number
  createdDate: string
  lastModifiedDate: string
}

/** The subset of a binding the edit dialog works on; a freshly bound entry
 * (before any reload) satisfies it too. */
export type TrackerLinkRef = Pick<
  TrackerLink,
  'seriesId' | 'provider' | 'trackId' | 'title' | 'trackMode' | 'chapterOffset'
>

/** Per-user display preferences: the series-detail tracker module only
 * renders in these libraries; empty means every library. */
export interface TrackerPreferences {
  libraries: string[]
  defaultTracker?: KomfOAuthProvider
}

export interface TrackerLinkUpsert {
  provider: KomfOAuthProvider
  trackId: string
  title?: string
  trackMode?: TrackerTrackMode
  chapterOffset?: number
}

// komf's TrackSearchItem schema, passed through verbatim
export interface TrackerSearchItem {
  id: string
  title: string
  coverUrl?: string
  description?: string
  tracked: boolean
  /** manga/novel distinction when the platform provides it */
  mediaType?: 'manga' | 'novel'
  url?: string
}

// komf's TrackState schema, passed through verbatim; totals are null while the
// platform entry carries no total (e.g. publishing)
export interface TrackerState {
  score?: number | null
  status?: string | null
  lastReadChapter?: number | null
  lastReadVolume?: number | null
  totalChapters?: number | null
  totalVolumes?: number | null
  startReadDate?: string | null
  finishReadDate?: string | null
}

export interface TrackerUpdatePayload {
  trackId: string
  score?: number
  status?: string
  lastReadChapter?: number
  lastReadVolume?: number
  startReadDate?: string | null
  finishReadDate?: string | null
}

// ---- Komf configuration ----

export type KomfLibraryType = 'MANGA' | 'NOVEL' | 'COMIC' | 'WEBTOON'
export type KomfUpdateMode = 'API' | 'COMIC_INFO' | 'MYLAR_SERIES_JSON'
export type KomfReadingDirection = 'LEFT_TO_RIGHT' | 'RIGHT_TO_LEFT' | 'VERTICAL' | 'WEBTOON'
export type KomfNameMatchingMode = 'EXACT' | 'CLOSEST_MATCH'
export type KomfAuthorRole =
  | 'WRITER'
  | 'PENCILLER'
  | 'INKER'
  | 'COLORIST'
  | 'LETTERER'
  | 'COVER'
  | 'EDITOR'
  | 'TRANSLATOR'
export type KomfMangaDexLink =
  | 'MANGA_DEX'
  | 'ANILIST'
  | 'ANIME_PLANET'
  | 'BOOKWALKER_JP'
  | 'MANGA_UPDATES'
  | 'NOVEL_UPDATES'
  | 'KITSU'
  | 'AMAZON'
  | 'EBOOK_JAPAN'
  | 'MY_ANIME_LIST'
  | 'CD_JAPAN'
  | 'RAW'
  | 'ENGLISH_TL'
export type KomfMangaBakaMode = 'API' | 'DATABASE'
export type KomfChineseDirection = 't2s' | 's2t'
export type KomfChineseField = 'title' | 'genres' | 'tags' | 'summary'

export interface KomfEventListenerConfig {
  enabled: boolean
  /** library ids; empty means all libraries */
  metadataLibraryFilter: string[]
  /** series ids excluded from automatic metadata updates */
  metadataSeriesExcludeFilter: string[]
  /** library ids; empty means all libraries */
  notificationsLibraryFilter: string[]
}

export interface KomfPublisherTagName {
  tagName: string
  language: string
}

export interface KomfAlternateTitleLabels {
  romaji: string | null
  native: string | null
  localized: string | null
}

export interface KomfPostProcessingConfig {
  seriesTitle: boolean
  seriesTitleLanguage: string | null
  alternativeSeriesTitles: boolean
  alternativeSeriesTitleLanguages: string[]
  orderBooks: boolean
  readingDirectionValue: KomfReadingDirection | null
  languageValue: string | null
  fallbackToAltTitle: boolean
  scoreTagName: string | null
  originalPublisherTagName: string | null
  publisherTagNames: KomfPublisherTagName[]
  /** komf-rs extensions */
  alternateTitleLabels?: KomfAlternateTitleLabels | null
  linksSkipEnabled?: boolean
  linksMatchEnabled?: boolean
}

export interface KomfSearchTitleExtractionConfig {
  enabled: boolean
  bracketRegex: string | null
  authorSeparator: string | null
  titleSplitters: string[]
  symbolNormalizeRegex: string | null
  charMappings: Array<[string, string]>
  cleanupRegex: string[]
}

export interface KomfChineseConversionConfig {
  enabled: boolean
  direction: KomfChineseDirection
  search: boolean
  matching: boolean
  update: {
    enabled: boolean
    fields: KomfChineseField[]
  }
}

export interface KomfMetadataProcessingConfig {
  libraryType: KomfLibraryType
  aggregate: boolean
  mergeTags: boolean
  mergeGenres: boolean
  bookCovers: boolean
  seriesCovers: boolean
  overrideExistingCovers: boolean
  lockCovers: boolean
  updateModes: KomfUpdateMode[]
  overrideComicInfo: boolean
  postProcessing: KomfPostProcessingConfig
  /** komf-rs extensions */
  mylarCovers?: boolean
  mylarOutputDir?: string | null
  searchTitleExtraction?: KomfSearchTitleExtractionConfig
  failedMatchCollectionName?: string | null
  chineseConversion?: KomfChineseConversionConfig
}

export interface KomfSeriesMetadataConfig {
  status: boolean
  title: boolean
  /** komf-rs extension */
  alternativeTitles?: boolean
  summary: boolean
  publisher: boolean
  readingDirection: boolean
  ageRating: boolean
  language: boolean
  genres: boolean
  tags: boolean
  totalBookCount: boolean
  authors: boolean
  releaseDate: boolean
  thumbnail: boolean
  links: boolean
  books: boolean
  /** komf-rs extension */
  score?: boolean
  useOriginalPublisher: boolean
  originalPublisherTagName?: string | null
  englishPublisherTagName?: string | null
  frenchPublisherTagName?: string | null
}

export interface KomfBookMetadataConfig {
  title: boolean
  summary: boolean
  number: boolean
  numberSort?: boolean
  releaseDate: boolean
  authors: boolean
  tags: boolean
  isbn: boolean
  links: boolean
  thumbnail: boolean
}

export interface KomfBangumiArchiveConfig {
  enabled: boolean
  dir?: string | null
  updateIntervalHours?: number
  idleReleaseSecs?: number | null
  staffChineseNames?: boolean
}

export interface KomfEHentaiArchiveConfig {
  enabled: boolean
  url?: string | null
  dbFile?: string | null
  updateIntervalHours?: number
  idleReleaseSecs?: number | null
  searchCategoryFilter?: string[]
  searchUploaderFilter?: string[]
}

/** Single provider block; the optional extras apply only to specific providers. */
export interface KomfProviderConfig {
  enabled: boolean
  priority: number
  seriesMetadata: KomfSeriesMetadataConfig
  /** null for providers without book-level metadata (aniList, mangaBaka) */
  bookMetadata: KomfBookMetadataConfig | null
  /** null inherits the global name matching mode */
  nameMatchingMode: KomfNameMatchingMode | null
  mediaType: KomfLibraryType | null
  authorRoles: KomfAuthorRole[]
  artistRoles: KomfAuthorRole[]
  /** bangumi */
  tagWhitelist?: string[]
  tagWhitelistFile?: string | null
  /** aniList */
  tagsScoreThreshold?: number
  tagsSizeLimit?: number
  titleLanguagePriority?: string[]
  /** mangaDex / mangaBaka */
  coverLanguages?: string[]
  links?: KomfMangaDexLink[]
  /** mangaBaka */
  mode?: KomfMangaBakaMode
  /** mangaBaka / bookWalker local database update interval in hours; 0 = manual only (komf-rs extension) */
  updateIntervalHours?: number
  /** bangumi / eHentai offline archive (komf-rs extension) */
  archive?: KomfBangumiArchiveConfig | KomfEHentaiArchiveConfig
  /** eHentai */
  preferredLanguages?: string[]
  titlePriority?: string
  translatorKeywords?: string[]
  maleOnlyTagsFile?: string | null
  titleTemplate?: string
  tagTranslationEnabled?: boolean
  tagTranslationUrl?: string
  gidOnlyMatch?: boolean
  searchDomain?: string
  ipbMemberId?: string | null
  ipbPassHash?: string | null
}

export interface KomfProvidersMap {
  mangaUpdates: KomfProviderConfig
  mangaBaka: KomfProviderConfig
  bookWalker: KomfProviderConfig
  mangaDex: KomfProviderConfig
  aniList: KomfProviderConfig
  mal: KomfProviderConfig
  comicVine: KomfProviderConfig
  nautiljon: KomfProviderConfig
  yenPress: KomfProviderConfig
  kodansha: KomfProviderConfig
  viz: KomfProviderConfig
  bangumi: KomfProviderConfig
  webtoons: KomfProviderConfig
  eHentai: KomfProviderConfig
}

export interface KomfMetadataProvidersConfig {
  malClientId: string | null
  comicVineClientId: string | null
  bangumiToken: string | null
  comicVineSearchLimit: number | null
  comicVineIssueName: string | null
  comicVineIdFormat: string | null
  nameMatchingMode: KomfNameMatchingMode
  defaultProviders: KomfProvidersMap
  /** null means the library has no override */
  libraryProviders: Record<string, KomfProvidersMap | null>
  /** read-only offline database info */
  mangaBakaDatabase?: { downloadTimestamp: string; checksum: string } | null
  bookWalkerDownloadDate?: string | null
}

export interface KomfNotificationConfig {
  discord?: {
    /** masked on read (e.g. "********"); patched by index, see KomfConfigPatch */
    webhooks?: string[] | null
    seriesCover: boolean
  }
  apprise?: {
    /** masked on read; patched by index, see KomfConfigPatch */
    urls?: string[] | null
    seriesCover: boolean
  }
}

export interface KomfConfig {
  komga: {
    eventListener: KomfEventListenerConfig
    metadataUpdate: {
      default: KomfMetadataProcessingConfig
      library: Record<string, KomfMetadataProcessingConfig>
    }
  }
  notifications?: KomfNotificationConfig
  metadataProviders: KomfMetadataProvidersConfig
}

export type KomfProcessingPatch = Partial<
  Omit<KomfMetadataProcessingConfig, 'postProcessing' | 'searchTitleExtraction' | 'chineseConversion'>
> & {
  postProcessing?: Partial<KomfPostProcessingConfig>
  searchTitleExtraction?: Partial<KomfSearchTitleExtractionConfig>
  chineseConversion?: Partial<Omit<KomfChineseConversionConfig, 'update'>> & {
    update?: Partial<KomfChineseConversionConfig['update']>
  }
}

export type KomfArchivePatch = Partial<KomfBangumiArchiveConfig & KomfEHentaiArchiveConfig>

export type KomfProviderPatch = Partial<
  Omit<KomfProviderConfig, 'seriesMetadata' | 'bookMetadata' | 'archive'>
> & {
  seriesMetadata?: Partial<KomfSeriesMetadataConfig>
  bookMetadata?: Partial<KomfBookMetadataConfig> | null
  archive?: KomfArchivePatch
}

type KomfProvidersPatch = { [K in keyof KomfProvidersMap]?: KomfProviderPatch }

/** Merge-patch body: absent field = keep, explicit null = clear, value = set. */
export interface KomfConfigPatch {
  komga?: {
    eventListener?: Partial<KomfEventListenerConfig>
    metadataUpdate?: {
      default?: KomfProcessingPatch
      /** null deletes the library's override */
      library?: Record<string, KomfProcessingPatch | null>
    }
  }
  notifications?: {
    discord?: {
      /** index-merge map: same index replaces, null deletes, a new index appends */
      webhooks?: Record<number, string | null>
      seriesCover?: boolean
    }
    apprise?: {
      urls?: Record<number, string | null>
      seriesCover?: boolean
    }
  }
  metadataProviders?: {
    malClientId?: string | null
    comicVineClientId?: string | null
    bangumiToken?: string | null
    comicVineSearchLimit?: number | null
    comicVineIssueName?: string | null
    comicVineIdFormat?: string | null
    nameMatchingMode?: KomfNameMatchingMode
    defaultProviders?: KomfProvidersPatch
    /** null deletes the library's override */
    libraryProviders?: Record<string, KomfProvidersPatch | null>
  }
}

export type KomfJobStatus = 'RUNNING' | 'FAILED' | 'COMPLETED'

export interface KomfJob {
  seriesId: string
  id: string
  status: KomfJobStatus
  message?: string
  startedAt: string
  finishedAt?: string
}

export interface KomfJobPage {
  content: KomfJob[]
  totalPages: number
  currentPage: number
}

export type KomfJobEvent = {
  /** the firehose (/komf/jobs/events) tags every frame with its job and series */
  jobId?: string
  seriesId?: string
} & (
  | { type: 'JobCreatedEvent'; startedAt?: string }
  | { type: 'JobFinishedEvent'; status: string; message?: string | null; finishedAt?: string }
  | { type: 'ProviderSeriesEvent'; provider: string }
  | { type: 'ProviderBookEvent'; provider: string; totalBooks: number; bookProgress: number }
  | { type: 'ProviderCompletedEvent'; provider: string }
  | { type: 'ProviderErrorEvent'; provider: string; message: string }
  | { type: 'PostProcessingStartEvent' }
  | { type: 'ProcessingErrorEvent'; message: string }
)

/** Readium position list; locator payloads stay opaque, @readium/shared parses them. */
export interface R2PositionsDto {
  total: number
  positions: unknown[]
}

export interface R2ProgressionDto {
  modified: string
  device: { id: string; name: string }
  locator: unknown
}

// kmrs-private reading statistics, not part of the Komga API surface
export interface NamedValueDto {
  name: string
  value: number
}

export interface ReadingSummaryDto {
  totalBooks: number
  booksStarted: number
  booksCompleted: number
  pagesRead: number
  averagePagesPerBook: number
  readingDays: number
  lastReadAt: string | null
  currentStreakDays: number
  longestStreakDays: number
  /** fixed read → inProgress → unread order, zero values included */
  statusDistribution: NamedValueDto[]
  generatedAt: string
}

export interface ReadingTimeSeriesPointDto {
  /** yyyy-MM-dd, UTC */
  date: string
  pagesRead: number
  booksCompleted: number
}

export interface ReadingActivityDto {
  /** 7 counts, index 0 = Sunday .. 6 = Saturday */
  weekdayDistribution: number[]
  /** 24 counts, index = hour of day */
  hourlyDistribution: number[]
  /** sparse: only days with activity, covering the full history */
  readingTimeSeries: ReadingTimeSeriesPointDto[]
  generatedAt: string
}

export interface ReadingTopsDto {
  topAuthors: NamedValueDto[]
  topGenres: NamedValueDto[]
  topTags: NamedValueDto[]
  genreDistribution: NamedValueDto[]
  tagDistribution: NamedValueDto[]
  generatedAt: string
}

// kmrs-private library/server statistics, not part of the Komga API surface
export interface LibraryStatsDto {
  libraryId: string
  name: string
  series: number
  books: number
  fileSize: number
  /** Admin-only, absent for other callers. */
  sidecars?: number
}

export interface LibrariesStatsDto {
  libraries: LibraryStatsDto[]
  /** sidecars is admin-only */
  total: {
    series: number
    books: number
    fileSize: number
    sidecars?: number
  }
}

export interface TaskTypeStatsDto {
  type: string
  queued: number
  executions: number
  totalTimeMs: number
  maxTimeMs: number
  failures: number
}

export interface ServerStatsDto {
  tasks: {
    queueSize: number
    /** queue depth and execution metrics merged by task type */
    types: TaskTypeStatsDto[]
  }
  process: {
    startTime: string
    uptimeSeconds: number
    /** percent of total CPU capacity: 100 means every core busy */
    cpuUsage: number
    memoryBytes: number
  }
  totals: {
    libraries: number
    /** absent on kmrs < 0.18.5 */
    series?: number
    books?: number
    fileSize?: number
    collections: number
    readlists: number
    sidecars: number
  }
}
