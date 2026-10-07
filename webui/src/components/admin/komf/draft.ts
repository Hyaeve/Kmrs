import type {
  KomfArchivePatch,
  KomfAuthorRole,
  KomfBookMetadataConfig,
  KomfChineseDirection,
  KomfChineseField,
  KomfConfig,
  KomfConfigPatch,
  KomfEventListenerConfig,
  KomfLibraryType,
  KomfMangaBakaMode,
  KomfMangaDexLink,
  KomfMetadataProcessingConfig,
  KomfNameMatchingMode,
  KomfPostProcessingConfig,
  KomfProcessingPatch,
  KomfProviderConfig,
  KomfProviderPatch,
  KomfProvidersMap,
  KomfPublisherTagName,
  KomfReadingDirection,
  KomfSeriesMetadataConfig,
  KomfUpdateMode,
} from '@/lib/api/types'

export const PROVIDER_KEYS = [
  'mangaUpdates',
  'mangaBaka',
  'bookWalker',
  'mangaDex',
  'aniList',
  'mal',
  'comicVine',
  'nautiljon',
  'yenPress',
  'kodansha',
  'viz',
  'bangumi',
  'webtoons',
  'eHentai',
] as const satisfies readonly (keyof KomfProvidersMap)[]

export type KomfProviderKey = (typeof PROVIDER_KEYS)[number]

export const PROVIDER_LABELS: Record<KomfProviderKey, string> = {
  mangaUpdates: 'MangaUpdates',
  mangaBaka: 'MangaBaka',
  bookWalker: 'BookWalker',
  mangaDex: 'MangaDex',
  aniList: 'AniList',
  mal: 'MAL',
  comicVine: 'ComicVine',
  nautiljon: 'Nautiljon',
  yenPress: 'Yen Press',
  kodansha: 'Kodansha',
  viz: 'Viz',
  bangumi: 'Bangumi',
  webtoons: 'Webtoons',
  eHentai: 'E-Hentai',
}

/** Providers with book-level metadata worth editing (mirrors the komf userscript). */
export const PROVIDERS_WITH_BOOKS: ReadonlySet<KomfProviderKey> = new Set([
  'mangaBaka',
  'nautiljon',
  'yenPress',
  'kodansha',
  'viz',
  'bookWalker',
  'mangaDex',
  'bangumi',
  'comicVine',
  'webtoons',
])

/** Providers that distinguish manga from novels. */
export const PROVIDERS_WITH_MEDIA_TYPE: ReadonlySet<KomfProviderKey> = new Set([
  'mangaBaka',
  'mangaUpdates',
  'mal',
  'nautiljon',
  'aniList',
  'yenPress',
  'bookWalker',
  'bangumi',
  'webtoons',
  'eHentai',
])

export const AUTHOR_ROLE_OPTIONS: KomfAuthorRole[] = [
  'WRITER',
  'PENCILLER',
  'INKER',
  'COLORIST',
  'LETTERER',
  'COVER',
  'EDITOR',
  'TRANSLATOR',
]

export const MANGADEX_LINK_OPTIONS: KomfMangaDexLink[] = [
  'MANGA_DEX',
  'ANILIST',
  'ANIME_PLANET',
  'BOOKWALKER_JP',
  'MANGA_UPDATES',
  'NOVEL_UPDATES',
  'KITSU',
  'AMAZON',
  'EBOOK_JAPAN',
  'MY_ANIME_LIST',
  'CD_JAPAN',
  'RAW',
  'ENGLISH_TL',
]

const DEFAULT_AUTHOR_ROLES: KomfAuthorRole[] = ['WRITER']
const DEFAULT_ARTIST_ROLES: KomfAuthorRole[] = ['PENCILLER', 'INKER', 'COLORIST', 'LETTERER', 'COVER']

// ---- Series / book metadata field toggles ----

export interface SeriesMetadataDraft {
  status: boolean
  title: boolean
  alternativeTitles: boolean
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
  score: boolean
  useOriginalPublisher: boolean
  originalPublisherTagName: string
  englishPublisherTagName: string
  frenchPublisherTagName: string
}

export interface BookMetadataDraft {
  title: boolean
  summary: boolean
  number: boolean
  numberSort: boolean
  releaseDate: boolean
  authors: boolean
  tags: boolean
  isbn: boolean
  links: boolean
  thumbnail: boolean
}

function seriesMetadataDraftFromConfig(s: KomfSeriesMetadataConfig | undefined): SeriesMetadataDraft {
  return {
    status: s?.status ?? true,
    title: s?.title ?? true,
    alternativeTitles: s?.alternativeTitles ?? true,
    summary: s?.summary ?? true,
    publisher: s?.publisher ?? true,
    readingDirection: s?.readingDirection ?? true,
    ageRating: s?.ageRating ?? true,
    language: s?.language ?? true,
    genres: s?.genres ?? true,
    tags: s?.tags ?? true,
    totalBookCount: s?.totalBookCount ?? true,
    authors: s?.authors ?? true,
    releaseDate: s?.releaseDate ?? true,
    thumbnail: s?.thumbnail ?? true,
    links: s?.links ?? true,
    books: s?.books ?? true,
    score: s?.score ?? false,
    useOriginalPublisher: s?.useOriginalPublisher ?? false,
    originalPublisherTagName: s?.originalPublisherTagName ?? '',
    englishPublisherTagName: s?.englishPublisherTagName ?? '',
    frenchPublisherTagName: s?.frenchPublisherTagName ?? '',
  }
}

function bookMetadataDraftFromConfig(b: KomfBookMetadataConfig | null | undefined): BookMetadataDraft {
  return {
    title: b?.title ?? true,
    summary: b?.summary ?? true,
    number: b?.number ?? true,
    numberSort: b?.numberSort ?? true,
    releaseDate: b?.releaseDate ?? true,
    authors: b?.authors ?? true,
    tags: b?.tags ?? true,
    isbn: b?.isbn ?? true,
    links: b?.links ?? true,
    thumbnail: b?.thumbnail ?? true,
  }
}

function seriesMetadataChanges(
  src: KomfSeriesMetadataConfig | undefined,
  d: SeriesMetadataDraft,
): Partial<KomfSeriesMetadataConfig> {
  const base = seriesMetadataDraftFromConfig(src)
  const out: Partial<KomfSeriesMetadataConfig> = {}
  if (d.status !== base.status) out.status = d.status
  if (d.title !== base.title) out.title = d.title
  if (d.alternativeTitles !== base.alternativeTitles) out.alternativeTitles = d.alternativeTitles
  if (d.summary !== base.summary) out.summary = d.summary
  if (d.publisher !== base.publisher) out.publisher = d.publisher
  if (d.readingDirection !== base.readingDirection) out.readingDirection = d.readingDirection
  if (d.ageRating !== base.ageRating) out.ageRating = d.ageRating
  if (d.language !== base.language) out.language = d.language
  if (d.genres !== base.genres) out.genres = d.genres
  if (d.tags !== base.tags) out.tags = d.tags
  if (d.totalBookCount !== base.totalBookCount) out.totalBookCount = d.totalBookCount
  if (d.authors !== base.authors) out.authors = d.authors
  if (d.releaseDate !== base.releaseDate) out.releaseDate = d.releaseDate
  if (d.thumbnail !== base.thumbnail) out.thumbnail = d.thumbnail
  if (d.links !== base.links) out.links = d.links
  if (d.books !== base.books) out.books = d.books
  if (d.score !== base.score) out.score = d.score
  if (d.useOriginalPublisher !== base.useOriginalPublisher) out.useOriginalPublisher = d.useOriginalPublisher
  const opn = d.originalPublisherTagName.trim()
  if (opn !== base.originalPublisherTagName) out.originalPublisherTagName = opn
  const epn = d.englishPublisherTagName.trim()
  if (epn !== base.englishPublisherTagName) out.englishPublisherTagName = epn
  const fpn = d.frenchPublisherTagName.trim()
  if (fpn !== base.frenchPublisherTagName) out.frenchPublisherTagName = fpn
  return out
}

function bookMetadataChanges(
  src: KomfBookMetadataConfig | null | undefined,
  d: BookMetadataDraft,
): Partial<KomfBookMetadataConfig> | undefined {
  const base = bookMetadataDraftFromConfig(src)
  const out: Partial<KomfBookMetadataConfig> = {}
  if (d.title !== base.title) out.title = d.title
  if (d.summary !== base.summary) out.summary = d.summary
  if (d.number !== base.number) out.number = d.number
  if (d.numberSort !== base.numberSort) out.numberSort = d.numberSort
  if (d.releaseDate !== base.releaseDate) out.releaseDate = d.releaseDate
  if (d.authors !== base.authors) out.authors = d.authors
  if (d.tags !== base.tags) out.tags = d.tags
  if (d.isbn !== base.isbn) out.isbn = d.isbn
  if (d.links !== base.links) out.links = d.links
  if (d.thumbnail !== base.thumbnail) out.thumbnail = d.thumbnail
  return Object.keys(out).length > 0 ? out : undefined
}

function seriesMetadataDraftToConfig(d: SeriesMetadataDraft): KomfSeriesMetadataConfig {
  return {
    status: d.status,
    title: d.title,
    alternativeTitles: d.alternativeTitles,
    summary: d.summary,
    publisher: d.publisher,
    readingDirection: d.readingDirection,
    ageRating: d.ageRating,
    language: d.language,
    genres: d.genres,
    tags: d.tags,
    totalBookCount: d.totalBookCount,
    authors: d.authors,
    releaseDate: d.releaseDate,
    thumbnail: d.thumbnail,
    links: d.links,
    books: d.books,
    score: d.score,
    useOriginalPublisher: d.useOriginalPublisher,
    originalPublisherTagName: d.originalPublisherTagName.trim(),
    englishPublisherTagName: d.englishPublisherTagName.trim(),
    frenchPublisherTagName: d.frenchPublisherTagName.trim(),
  }
}

function bookMetadataDraftToConfig(d: BookMetadataDraft): KomfBookMetadataConfig {
  return { ...d }
}

// ---- Provider drafts ----

export interface ProviderDraft {
  enabled: boolean
  priority: string
  mediaType: KomfLibraryType
  /** '' inherits the global name matching mode */
  nameMatchingMode: KomfNameMatchingMode | ''
  authorRoles: KomfAuthorRole[]
  artistRoles: KomfAuthorRole[]
  seriesMetadata: SeriesMetadataDraft
  bookMetadata: BookMetadataDraft
  /** aniList */
  tagsScoreThreshold: string
  tagsSizeLimit: string
  titleLanguagePriority: string[]
  /** mangaDex / mangaBaka */
  coverLanguages: string[]
  links: KomfMangaDexLink[]
  /** mangaBaka */
  mode: KomfMangaBakaMode
  /** mangaBaka / bookWalker local database update interval in hours */
  updateIntervalHours: string
  /** bangumi */
  tagWhitelist: string[]
  tagWhitelistFile: string
  /** bangumi / eHentai offline archive */
  archiveEnabled: boolean
  archiveDir: string
  archiveUrl: string
  archiveDbFile: string
  archiveUpdateIntervalHours: string
  archiveIdleReleaseSecs: string
  archiveStaffChineseNames: boolean
  archiveSearchCategoryFilter: string[]
  archiveSearchUploaderFilter: string[]
  /** eHentai */
  preferredLanguages: string[]
  titlePriority: string
  translatorKeywords: string[]
  maleOnlyTagsFile: string
  titleTemplate: string
  tagTranslationEnabled: boolean
  tagTranslationUrl: string
  gidOnlyMatch: boolean
  searchDomain: string
  ipbMemberId: string
  ipbPassHash: string
}

export type ProvidersDraft = Record<KomfProviderKey, ProviderDraft>

export function providerDraftFromConfig(key: KomfProviderKey, p: KomfProviderConfig | undefined): ProviderDraft {
  const archive = p?.archive
  const ehArchive = key === 'eHentai' ? (archive as KomfProviderConfig['archive']) : undefined
  return {
    enabled: p?.enabled ?? false,
    priority: (p?.priority ?? 10).toString(),
    mediaType: p?.mediaType ?? 'MANGA',
    nameMatchingMode: p?.nameMatchingMode ?? '',
    authorRoles: p?.authorRoles ? [...p.authorRoles] : [...DEFAULT_AUTHOR_ROLES],
    artistRoles: p?.artistRoles ? [...p.artistRoles] : [...DEFAULT_ARTIST_ROLES],
    seriesMetadata: seriesMetadataDraftFromConfig(p?.seriesMetadata),
    bookMetadata: bookMetadataDraftFromConfig(p?.bookMetadata),
    tagsScoreThreshold: (p?.tagsScoreThreshold ?? 60).toString(),
    tagsSizeLimit: (p?.tagsSizeLimit ?? 15).toString(),
    titleLanguagePriority: p?.titleLanguagePriority ? [...p.titleLanguagePriority] : [],
    coverLanguages: p?.coverLanguages ? [...p.coverLanguages] : ['en', 'ja'],
    links: p?.links ? [...p.links] : [...MANGADEX_LINK_OPTIONS],
    mode: p?.mode ?? 'API',
    updateIntervalHours: (p?.updateIntervalHours ?? 24).toString(),
    tagWhitelist: p?.tagWhitelist ? [...p.tagWhitelist] : [],
    tagWhitelistFile: p?.tagWhitelistFile ?? '',
    archiveEnabled: archive?.enabled ?? false,
    archiveDir: (archive && 'dir' in archive ? archive.dir : undefined) ?? '',
    archiveUrl: ehArchive && 'url' in ehArchive ? (ehArchive.url ?? '') : '',
    archiveDbFile: ehArchive && 'dbFile' in ehArchive ? (ehArchive.dbFile ?? '') : '',
    archiveUpdateIntervalHours: (archive?.updateIntervalHours ?? 168).toString(),
    archiveIdleReleaseSecs: (archive?.idleReleaseSecs ?? 60).toString(),
    archiveStaffChineseNames:
      archive && 'staffChineseNames' in archive ? (archive.staffChineseNames ?? false) : false,
    archiveSearchCategoryFilter:
      ehArchive && 'searchCategoryFilter' in ehArchive ? [...(ehArchive.searchCategoryFilter ?? [])] : [],
    archiveSearchUploaderFilter:
      ehArchive && 'searchUploaderFilter' in ehArchive ? [...(ehArchive.searchUploaderFilter ?? [])] : [],
    preferredLanguages: p?.preferredLanguages ? [...p.preferredLanguages] : ['en', 'ja'],
    titlePriority: p?.titlePriority ?? 'jpn',
    translatorKeywords: p?.translatorKeywords ? [...p.translatorKeywords] : [],
    maleOnlyTagsFile: p?.maleOnlyTagsFile ?? '',
    titleTemplate: p?.titleTemplate ?? '',
    tagTranslationEnabled: p?.tagTranslationEnabled ?? false,
    tagTranslationUrl: p?.tagTranslationUrl ?? '',
    gidOnlyMatch: p?.gidOnlyMatch ?? false,
    searchDomain: p?.searchDomain ?? 'e-hentai',
    ipbMemberId: p?.ipbMemberId ?? '',
    ipbPassHash: p?.ipbPassHash ?? '',
  }
}

export function providersDraftFromMap(m: KomfProvidersMap): ProvidersDraft {
  const out = {} as ProvidersDraft
  for (const key of PROVIDER_KEYS) out[key] = providerDraftFromConfig(key, m[key])
  return out
}

// ---- Processing drafts ----

export interface SearchTitleExtractionDraft {
  enabled: boolean
  bracketRegex: string
  authorSeparator: string
  titleSplitters: string[]
  symbolNormalizeRegex: string
  charMappings: Array<{ from: string; to: string }>
  cleanupRegex: string[]
}

export interface ChineseConversionDraft {
  enabled: boolean
  direction: KomfChineseDirection
  search: boolean
  matching: boolean
  updateEnabled: boolean
  updateFields: KomfChineseField[]
}

const DEFAULT_SYMBOL_NORMALIZE_REGEX = "[:：•·․,，。'’?？!！~⁓～]"

/** Editable form mirror of a processing block: text inputs are strings, '' means "no value / clear". */
export interface ProcessingDraft {
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
  seriesTitle: boolean
  seriesTitleLanguage: string
  alternativeSeriesTitles: boolean
  alternativeSeriesTitleLanguages: string[]
  orderBooks: boolean
  readingDirectionValue: KomfReadingDirection | ''
  languageValue: string
  fallbackToAltTitle: boolean
  scoreTagName: string
  originalPublisherTagName: string
  publisherTagNames: KomfPublisherTagName[]
  alternateTitleLabels: { romaji: string; native: string; localized: string }
  linksSkipEnabled: boolean
  linksMatchEnabled: boolean
  mylarCovers: boolean
  mylarOutputDir: string
  failedMatchCollectionName: string
  searchTitleExtraction: SearchTitleExtractionDraft
  chineseConversion: ChineseConversionDraft
}

export function processingDraftFromConfig(p: KomfMetadataProcessingConfig): ProcessingDraft {
  const ste = p.searchTitleExtraction
  const cc = p.chineseConversion
  return {
    libraryType: p.libraryType,
    aggregate: p.aggregate,
    mergeTags: p.mergeTags,
    mergeGenres: p.mergeGenres,
    bookCovers: p.bookCovers,
    seriesCovers: p.seriesCovers,
    overrideExistingCovers: p.overrideExistingCovers,
    lockCovers: p.lockCovers,
    updateModes: [...p.updateModes],
    overrideComicInfo: p.overrideComicInfo,
    seriesTitle: p.postProcessing.seriesTitle,
    seriesTitleLanguage: p.postProcessing.seriesTitleLanguage ?? '',
    alternativeSeriesTitles: p.postProcessing.alternativeSeriesTitles,
    alternativeSeriesTitleLanguages: [...(p.postProcessing.alternativeSeriesTitleLanguages ?? [])],
    orderBooks: p.postProcessing.orderBooks,
    readingDirectionValue: p.postProcessing.readingDirectionValue ?? '',
    languageValue: p.postProcessing.languageValue ?? '',
    fallbackToAltTitle: p.postProcessing.fallbackToAltTitle ?? false,
    scoreTagName: p.postProcessing.scoreTagName ?? '',
    originalPublisherTagName: p.postProcessing.originalPublisherTagName ?? '',
    publisherTagNames: (p.postProcessing.publisherTagNames ?? []).map((t) => ({ ...t })),
    alternateTitleLabels: {
      romaji: p.postProcessing.alternateTitleLabels?.romaji ?? '',
      native: p.postProcessing.alternateTitleLabels?.native ?? '',
      localized: p.postProcessing.alternateTitleLabels?.localized ?? '',
    },
    linksSkipEnabled: p.postProcessing.linksSkipEnabled ?? true,
    linksMatchEnabled: p.postProcessing.linksMatchEnabled ?? true,
    mylarCovers: p.mylarCovers ?? false,
    mylarOutputDir: p.mylarOutputDir ?? '',
    failedMatchCollectionName: p.failedMatchCollectionName ?? '',
    searchTitleExtraction: {
      enabled: ste?.enabled ?? false,
      bracketRegex: ste?.bracketRegex ?? '',
      authorSeparator: ste?.authorSeparator ?? '',
      titleSplitters: ste?.titleSplitters ? [...ste.titleSplitters] : [],
      symbolNormalizeRegex: ste?.symbolNormalizeRegex ?? DEFAULT_SYMBOL_NORMALIZE_REGEX,
      charMappings: (ste?.charMappings ?? []).map(([from, to]) => ({ from, to })),
      cleanupRegex: ste?.cleanupRegex ? [...ste.cleanupRegex] : [],
    },
    chineseConversion: {
      enabled: cc?.enabled ?? false,
      direction: cc?.direction ?? 't2s',
      search: cc?.search ?? true,
      matching: cc?.matching ?? true,
      updateEnabled: cc?.update?.enabled ?? true,
      updateFields: cc?.update?.fields ? [...cc.update.fields] : ['title'],
    },
  }
}

// ---- Notification drafts ----

export interface NotificationUrlEntry {
  /** index in the server-side list; null for rows added in this session */
  key: number | null
  value: string
}

export interface NotificationsDraft {
  discordSeriesCover: boolean
  discordWebhooks: NotificationUrlEntry[]
  appriseSeriesCover: boolean
  appriseUrls: NotificationUrlEntry[]
}

// ---- Config draft ----

export interface KomfConfigDraft {
  eventListenerEnabled: boolean
  eventListenerLibraryFilter: string[]
  eventListenerSeriesExcludeFilter: string[]
  notificationsLibraryFilter: string[]
  defaultProviders: ProvidersDraft
  nameMatchingMode: KomfNameMatchingMode
  malClientId: string
  comicVineApiKey: string
  comicVineSearchLimit: string
  comicVineIssueName: string
  comicVineIdFormat: string
  bangumiToken: string
  defaultProcessing: ProcessingDraft
  notifications: NotificationsDraft
  /** key present = override enabled for that library */
  libraryProcessing: Record<string, ProcessingDraft>
  libraryProviders: Record<string, ProvidersDraft>
}

export function draftFromConfig(config: KomfConfig): KomfConfigDraft {
  const notifications = config.notifications
  return {
    eventListenerEnabled: config.komga.eventListener.enabled,
    eventListenerLibraryFilter: [...config.komga.eventListener.metadataLibraryFilter],
    eventListenerSeriesExcludeFilter: [...(config.komga.eventListener.metadataSeriesExcludeFilter ?? [])],
    notificationsLibraryFilter: [...(config.komga.eventListener.notificationsLibraryFilter ?? [])],
    defaultProviders: providersDraftFromMap(config.metadataProviders.defaultProviders),
    nameMatchingMode: config.metadataProviders.nameMatchingMode,
    malClientId: config.metadataProviders.malClientId ?? '',
    comicVineApiKey: config.metadataProviders.comicVineClientId ?? '',
    comicVineSearchLimit: config.metadataProviders.comicVineSearchLimit?.toString() ?? '',
    comicVineIssueName: config.metadataProviders.comicVineIssueName ?? '',
    comicVineIdFormat: config.metadataProviders.comicVineIdFormat ?? '',
    bangumiToken: config.metadataProviders.bangumiToken ?? '',
    defaultProcessing: processingDraftFromConfig(config.komga.metadataUpdate.default),
    notifications: {
      discordSeriesCover: notifications?.discord?.seriesCover ?? false,
      // komf masks existing webhook URLs on read, so they are kept by index and never re-sent
      discordWebhooks: (notifications?.discord?.webhooks ?? []).map((v, i) => ({ key: i, value: v })),
      appriseSeriesCover: notifications?.apprise?.seriesCover ?? false,
      appriseUrls: (notifications?.apprise?.urls ?? []).map((v, i) => ({ key: i, value: v })),
    },
    libraryProcessing: Object.fromEntries(
      Object.entries(config.komga.metadataUpdate.library).map(([id, p]) => [id, processingDraftFromConfig(p)]),
    ),
    libraryProviders: Object.fromEntries(
      Object.entries(config.metadataProviders.libraryProviders)
        .filter((e): e is [string, KomfProvidersMap] => e[1] !== null)
        .map(([id, m]) => [id, providersDraftFromMap(m)]),
    ),
  }
}

// ---- Validation ----

export interface KomfDraftErrors {
  defaultProviders: Partial<Record<KomfProviderKey, string>>
  libraryProviders: Record<string, Partial<Record<KomfProviderKey, string>>>
  comicVineSearchLimit?: string
  messages: string[]
  /** per-scope attribution of the processing messages, so each tab can badge its own errors */
  defaultProcessingMessages: string[]
  libraryProcessingMessages: string[]
}

function positiveInt(v: string): boolean {
  return /^\d+$/.test(v) && Number(v) > 0
}

function nonNegativeInt(v: string): boolean {
  return /^\d+$/.test(v)
}

// validation results are admin-komf message keys, translated at render
const PRIORITY_ERROR = 'validation.positiveInt'
const NUMBER_ERROR = 'validation.wholeNumber'

function validateProvider(key: KomfProviderKey, d: ProviderDraft): string | undefined {
  if (!positiveInt(d.priority.trim())) return PRIORITY_ERROR
  if (key === 'aniList') {
    if (!nonNegativeInt(d.tagsScoreThreshold.trim()) || !nonNegativeInt(d.tagsSizeLimit.trim())) return NUMBER_ERROR
  }
  if (key === 'mangaBaka' || key === 'bookWalker') {
    if (!nonNegativeInt(d.updateIntervalHours.trim())) return NUMBER_ERROR
  }
  if (key === 'bangumi' || key === 'eHentai') {
    if (!nonNegativeInt(d.archiveUpdateIntervalHours.trim()) || !nonNegativeInt(d.archiveIdleReleaseSecs.trim()))
      return NUMBER_ERROR
  }
  return undefined
}

export function validateDraft(d: KomfConfigDraft): KomfDraftErrors {
  const errors: KomfDraftErrors = {
    defaultProviders: {},
    libraryProviders: {},
    messages: [],
    defaultProcessingMessages: [],
    libraryProcessingMessages: [],
  }
  for (const key of PROVIDER_KEYS) {
    const provider = d.defaultProviders[key]
    // disabled providers are hidden in the UI; their inert settings must not block saving
    if (!provider.enabled) continue
    const error = validateProvider(key, provider)
    if (error) errors.defaultProviders[key] = error
  }
  for (const [libId, providers] of Object.entries(d.libraryProviders)) {
    const sub: Partial<Record<KomfProviderKey, string>> = {}
    for (const key of PROVIDER_KEYS) {
      if (!providers[key].enabled) continue
      const error = validateProvider(key, providers[key])
      if (error) sub[key] = error
    }
    if (Object.keys(sub).length > 0) errors.libraryProviders[libId] = sub
  }
  const cvs = d.comicVineSearchLimit.trim()
  if (cvs !== '' && !positiveInt(cvs)) errors.comicVineSearchLimit = PRIORITY_ERROR

  const checkProcessing = (p: ProcessingDraft): string[] => {
    const msgs: string[] = []
    const add = (msg: string) => {
      if (!msgs.includes(msg)) msgs.push(msg)
    }
    for (const t of p.publisherTagNames) {
      if (!t.tagName.trim() !== !t.language.trim()) add('validation.publisherTagPair')
    }
    for (const m of p.searchTitleExtraction.charMappings) {
      if (!m.from.trim() !== !m.to.trim()) add('validation.charMappingPair')
    }
    return msgs
  }
  errors.defaultProcessingMessages = checkProcessing(d.defaultProcessing)
  errors.libraryProcessingMessages = Object.values(d.libraryProcessing).flatMap(checkProcessing)
  errors.messages = [...new Set([...errors.defaultProcessingMessages, ...errors.libraryProcessingMessages])]

  return errors
}

export function hasErrors(errors: KomfDraftErrors): boolean {
  return (
    Object.keys(errors.defaultProviders).length > 0 ||
    Object.keys(errors.libraryProviders).length > 0 ||
    errors.comicVineSearchLimit !== undefined ||
    errors.messages.length > 0
  )
}

// ---- Change computation ----

function sameStringSet(a: string[], b: string[]): boolean {
  return a.length === b.length && a.every((x) => b.includes(x))
}

function sameStringArray(a: string[], b: string[]): boolean {
  return a.length === b.length && a.every((x, i) => b[i] === x)
}

/** Textarea-backed lists keep raw lines in the draft; empty/blank lines are not values. */
function cleanList(v: string[]): string[] {
  return v.map((x) => x.trim()).filter((x) => x !== '')
}

type ProvidersPatch = { [K in KomfProviderKey]?: KomfProviderPatch }

function nonEmpty(t: string): boolean {
  return t.trim() !== ''
}

function completePublisherTagNames(rows: KomfPublisherTagName[]): KomfPublisherTagName[] {
  return rows.filter((t) => nonEmpty(t.tagName) && nonEmpty(t.language))
}

function charMappingsToTuples(rows: Array<{ from: string; to: string }>): Array<[string, string]> {
  return rows.filter((m) => nonEmpty(m.from) && nonEmpty(m.to)).map((m) => [m.from, m.to])
}

/** PATCH semantics: only dirty fields, '' on a tri-state text field clears it (null). */
function processingChanges(src: KomfMetadataProcessingConfig, d: ProcessingDraft): KomfProcessingPatch {
  const out: KomfProcessingPatch = {}
  if (d.libraryType !== src.libraryType) out.libraryType = d.libraryType
  if (d.aggregate !== src.aggregate) out.aggregate = d.aggregate
  if (d.mergeTags !== src.mergeTags) out.mergeTags = d.mergeTags
  if (d.mergeGenres !== src.mergeGenres) out.mergeGenres = d.mergeGenres
  if (d.bookCovers !== src.bookCovers) out.bookCovers = d.bookCovers
  if (d.seriesCovers !== src.seriesCovers) out.seriesCovers = d.seriesCovers
  if (d.overrideExistingCovers !== src.overrideExistingCovers) out.overrideExistingCovers = d.overrideExistingCovers
  if (d.lockCovers !== src.lockCovers) out.lockCovers = d.lockCovers
  if (!sameStringSet(d.updateModes, src.updateModes)) out.updateModes = d.updateModes
  if (d.overrideComicInfo !== src.overrideComicInfo) out.overrideComicInfo = d.overrideComicInfo
  if (d.mylarCovers !== (src.mylarCovers ?? false)) out.mylarCovers = d.mylarCovers
  const mod = d.mylarOutputDir.trim()
  if (mod !== (src.mylarOutputDir ?? '')) out.mylarOutputDir = mod === '' ? null : mod
  const fmc = d.failedMatchCollectionName.trim()
  if (fmc !== (src.failedMatchCollectionName ?? '')) out.failedMatchCollectionName = fmc === '' ? null : fmc

  const pp = src.postProcessing
  const post: Partial<KomfPostProcessingConfig> = {}
  if (d.seriesTitle !== pp.seriesTitle) post.seriesTitle = d.seriesTitle
  const stl = d.seriesTitleLanguage.trim()
  if (stl !== (pp.seriesTitleLanguage ?? '')) post.seriesTitleLanguage = stl === '' ? null : stl
  if (d.alternativeSeriesTitles !== pp.alternativeSeriesTitles)
    post.alternativeSeriesTitles = d.alternativeSeriesTitles
  if (!sameStringSet(cleanList(d.alternativeSeriesTitleLanguages), pp.alternativeSeriesTitleLanguages ?? []))
    post.alternativeSeriesTitleLanguages = cleanList(d.alternativeSeriesTitleLanguages)
  if (d.orderBooks !== pp.orderBooks) post.orderBooks = d.orderBooks
  if (d.readingDirectionValue !== (pp.readingDirectionValue ?? ''))
    post.readingDirectionValue = d.readingDirectionValue === '' ? null : d.readingDirectionValue
  const lv = d.languageValue.trim()
  if (lv !== (pp.languageValue ?? '')) post.languageValue = lv === '' ? null : lv
  if (d.fallbackToAltTitle !== (pp.fallbackToAltTitle ?? false)) post.fallbackToAltTitle = d.fallbackToAltTitle
  const stn = d.scoreTagName.trim()
  if (stn !== (pp.scoreTagName ?? '')) post.scoreTagName = stn === '' ? null : stn
  const opn = d.originalPublisherTagName.trim()
  if (opn !== (pp.originalPublisherTagName ?? '')) post.originalPublisherTagName = opn === '' ? null : opn
  const ptn = completePublisherTagNames(d.publisherTagNames)
  if (JSON.stringify(ptn) !== JSON.stringify(pp.publisherTagNames ?? [])) post.publisherTagNames = ptn
  const atl = d.alternateTitleLabels
  const srcAtl = pp.alternateTitleLabels
  if (
    atl.romaji.trim() !== (srcAtl?.romaji ?? '') ||
    atl.native.trim() !== (srcAtl?.native ?? '') ||
    atl.localized.trim() !== (srcAtl?.localized ?? '')
  ) {
    post.alternateTitleLabels = {
      romaji: atl.romaji.trim() === '' ? null : atl.romaji.trim(),
      native: atl.native.trim() === '' ? null : atl.native.trim(),
      localized: atl.localized.trim() === '' ? null : atl.localized.trim(),
    }
  }
  if (d.linksSkipEnabled !== (pp.linksSkipEnabled ?? true)) post.linksSkipEnabled = d.linksSkipEnabled
  if (d.linksMatchEnabled !== (pp.linksMatchEnabled ?? true)) post.linksMatchEnabled = d.linksMatchEnabled
  if (Object.keys(post).length > 0) out.postProcessing = post

  const ste = d.searchTitleExtraction
  const srcExt = processingDraftFromConfig(src)
  const srcSte = srcExt.searchTitleExtraction
  const stePatch: NonNullable<KomfProcessingPatch['searchTitleExtraction']> = {}
  if (ste.enabled !== srcSte.enabled) stePatch.enabled = ste.enabled
  const br = ste.bracketRegex.trim()
  if (br !== srcSte.bracketRegex) stePatch.bracketRegex = br === '' ? null : br
  const as = ste.authorSeparator.trim()
  if (as !== srcSte.authorSeparator) stePatch.authorSeparator = as === '' ? null : as
  if (!sameStringArray(cleanList(ste.titleSplitters), srcSte.titleSplitters))
    stePatch.titleSplitters = cleanList(ste.titleSplitters)
  const snr = ste.symbolNormalizeRegex.trim()
  if (snr !== srcSte.symbolNormalizeRegex) stePatch.symbolNormalizeRegex = snr
  const cm = charMappingsToTuples(ste.charMappings)
  if (JSON.stringify(cm) !== JSON.stringify(charMappingsToTuples(srcSte.charMappings))) stePatch.charMappings = cm
  if (!sameStringArray(cleanList(ste.cleanupRegex), srcSte.cleanupRegex)) stePatch.cleanupRegex = cleanList(ste.cleanupRegex)
  if (Object.keys(stePatch).length > 0) out.searchTitleExtraction = stePatch

  const cc = d.chineseConversion
  const srcCc = srcExt.chineseConversion
  const ccPatch: NonNullable<KomfProcessingPatch['chineseConversion']> = {}
  if (cc.enabled !== srcCc.enabled) ccPatch.enabled = cc.enabled
  if (cc.direction !== srcCc.direction) ccPatch.direction = cc.direction
  if (cc.search !== srcCc.search) ccPatch.search = cc.search
  if (cc.matching !== srcCc.matching) ccPatch.matching = cc.matching
  if (cc.updateEnabled !== srcCc.updateEnabled || !sameStringSet(cc.updateFields, srcCc.updateFields)) {
    ccPatch.update = {}
    if (cc.updateEnabled !== srcCc.updateEnabled) ccPatch.update.enabled = cc.updateEnabled
    if (!sameStringSet(cc.updateFields, srcCc.updateFields)) ccPatch.update.fields = cc.updateFields
  }
  if (Object.keys(ccPatch).length > 0) out.chineseConversion = ccPatch

  return out
}

/** A brand-new override sends the whole draft (komf would otherwise seed its own defaults). */
function processingDraftToPatch(d: ProcessingDraft): KomfProcessingPatch {
  const stl = d.seriesTitleLanguage.trim()
  const lv = d.languageValue.trim()
  const stn = d.scoreTagName.trim()
  const opn = d.originalPublisherTagName.trim()
  const mod = d.mylarOutputDir.trim()
  const fmc = d.failedMatchCollectionName.trim()
  const atl = d.alternateTitleLabels
  const ste = d.searchTitleExtraction
  return {
    libraryType: d.libraryType,
    aggregate: d.aggregate,
    mergeTags: d.mergeTags,
    mergeGenres: d.mergeGenres,
    bookCovers: d.bookCovers,
    seriesCovers: d.seriesCovers,
    overrideExistingCovers: d.overrideExistingCovers,
    lockCovers: d.lockCovers,
    updateModes: d.updateModes,
    overrideComicInfo: d.overrideComicInfo,
    mylarCovers: d.mylarCovers,
    mylarOutputDir: mod === '' ? null : mod,
    failedMatchCollectionName: fmc === '' ? null : fmc,
    postProcessing: {
      seriesTitle: d.seriesTitle,
      seriesTitleLanguage: stl === '' ? null : stl,
      alternativeSeriesTitles: d.alternativeSeriesTitles,
      alternativeSeriesTitleLanguages: cleanList(d.alternativeSeriesTitleLanguages),
      orderBooks: d.orderBooks,
      readingDirectionValue: d.readingDirectionValue === '' ? null : d.readingDirectionValue,
      languageValue: lv === '' ? null : lv,
      fallbackToAltTitle: d.fallbackToAltTitle,
      scoreTagName: stn === '' ? null : stn,
      originalPublisherTagName: opn === '' ? null : opn,
      publisherTagNames: completePublisherTagNames(d.publisherTagNames),
      alternateTitleLabels: {
        romaji: atl.romaji.trim() === '' ? null : atl.romaji.trim(),
        native: atl.native.trim() === '' ? null : atl.native.trim(),
        localized: atl.localized.trim() === '' ? null : atl.localized.trim(),
      },
      linksSkipEnabled: d.linksSkipEnabled,
      linksMatchEnabled: d.linksMatchEnabled,
    },
    searchTitleExtraction: {
      enabled: ste.enabled,
      bracketRegex: ste.bracketRegex.trim() === '' ? null : ste.bracketRegex.trim(),
      authorSeparator: ste.authorSeparator.trim() === '' ? null : ste.authorSeparator.trim(),
      titleSplitters: cleanList(ste.titleSplitters),
      symbolNormalizeRegex: ste.symbolNormalizeRegex.trim(),
      charMappings: charMappingsToTuples(ste.charMappings),
      cleanupRegex: cleanList(ste.cleanupRegex),
    },
    chineseConversion: {
      enabled: d.chineseConversion.enabled,
      direction: d.chineseConversion.direction,
      search: d.chineseConversion.search,
      matching: d.chineseConversion.matching,
      update: {
        enabled: d.chineseConversion.updateEnabled,
        fields: d.chineseConversion.updateFields,
      },
    },
  }
}

function providerChanges(key: KomfProviderKey, src: KomfProviderConfig, d: ProviderDraft): KomfProviderPatch {
  const out: KomfProviderPatch = {}
  if (d.enabled !== src.enabled) out.enabled = d.enabled
  const p = d.priority.trim()
  if (p !== src.priority.toString() && positiveInt(p)) out.priority = Number(p)

  const sm = seriesMetadataChanges(src.seriesMetadata, d.seriesMetadata)
  if (Object.keys(sm).length > 0) out.seriesMetadata = sm

  if (PROVIDERS_WITH_BOOKS.has(key)) {
    const bm = bookMetadataChanges(src.bookMetadata, d.bookMetadata)
    if (bm) out.bookMetadata = bm
  }

  if (d.nameMatchingMode !== (src.nameMatchingMode ?? ''))
    out.nameMatchingMode = d.nameMatchingMode === '' ? null : d.nameMatchingMode

  if (PROVIDERS_WITH_MEDIA_TYPE.has(key) && d.mediaType !== (src.mediaType ?? 'MANGA')) out.mediaType = d.mediaType

  if (!sameStringSet(d.authorRoles, src.authorRoles ?? DEFAULT_AUTHOR_ROLES)) out.authorRoles = d.authorRoles
  if (!sameStringSet(d.artistRoles, src.artistRoles ?? DEFAULT_ARTIST_ROLES)) out.artistRoles = d.artistRoles

  if (key === 'aniList') {
    const tst = d.tagsScoreThreshold.trim()
    if (tst !== (src.tagsScoreThreshold ?? 60).toString() && nonNegativeInt(tst)) out.tagsScoreThreshold = Number(tst)
    const tsl = d.tagsSizeLimit.trim()
    if (tsl !== (src.tagsSizeLimit ?? 15).toString() && nonNegativeInt(tsl)) out.tagsSizeLimit = Number(tsl)
    // ordered priority list: membership-only comparison would silently drop reorders
    if (!sameStringArray(cleanList(d.titleLanguagePriority), src.titleLanguagePriority ?? []))
      out.titleLanguagePriority = cleanList(d.titleLanguagePriority)
  }

  if (key === 'mangaDex' || key === 'mangaBaka') {
    if (!sameStringSet(cleanList(d.coverLanguages), src.coverLanguages ?? ['en', 'ja']))
      out.coverLanguages = cleanList(d.coverLanguages)
  }

  if (key === 'mangaDex') {
    if (!sameStringSet(d.links, src.links ?? MANGADEX_LINK_OPTIONS)) out.links = d.links
  }

  if (key === 'mangaBaka' || key === 'bookWalker') {
    const hours = d.updateIntervalHours.trim()
    if (hours !== (src.updateIntervalHours ?? 24).toString() && nonNegativeInt(hours))
      out.updateIntervalHours = Number(hours)
  }

  if (key === 'mangaBaka' && d.mode !== (src.mode ?? 'API')) out.mode = d.mode

  if (key === 'bangumi') {
    if (!sameStringSet(cleanList(d.tagWhitelist), src.tagWhitelist ?? [])) out.tagWhitelist = cleanList(d.tagWhitelist)
    const twf = d.tagWhitelistFile.trim()
    if (twf !== (src.tagWhitelistFile ?? '')) out.tagWhitelistFile = twf === '' ? null : twf
  }

  if (key === 'eHentai') {
    if (!sameStringSet(cleanList(d.preferredLanguages), src.preferredLanguages ?? ['en', 'ja']))
      out.preferredLanguages = cleanList(d.preferredLanguages)
    if (d.titlePriority !== (src.titlePriority ?? 'jpn')) out.titlePriority = d.titlePriority
    if (!sameStringSet(cleanList(d.translatorKeywords), src.translatorKeywords ?? []))
      out.translatorKeywords = cleanList(d.translatorKeywords)
    const mof = d.maleOnlyTagsFile.trim()
    if (mof !== (src.maleOnlyTagsFile ?? '')) out.maleOnlyTagsFile = mof === '' ? null : mof
    if (d.titleTemplate.trim() !== (src.titleTemplate ?? '')) out.titleTemplate = d.titleTemplate.trim()
    if (d.tagTranslationEnabled !== (src.tagTranslationEnabled ?? false))
      out.tagTranslationEnabled = d.tagTranslationEnabled
    // plain-Option field: null would be a no-op, so '' is sent through to reset to the default
    if (d.tagTranslationUrl.trim() !== (src.tagTranslationUrl ?? '')) out.tagTranslationUrl = d.tagTranslationUrl.trim()
    if (d.gidOnlyMatch !== (src.gidOnlyMatch ?? false)) out.gidOnlyMatch = d.gidOnlyMatch
    if (d.searchDomain !== (src.searchDomain ?? 'e-hentai')) out.searchDomain = d.searchDomain
    const imi = d.ipbMemberId.trim()
    if (imi !== (src.ipbMemberId ?? '')) out.ipbMemberId = imi === '' ? null : imi
    const iph = d.ipbPassHash.trim()
    if (iph !== (src.ipbPassHash ?? '')) out.ipbPassHash = iph === '' ? null : iph
  }

  if (key === 'bangumi' || key === 'eHentai') {
    const srcArchive = providerDraftFromConfig(key, src)
    const archive: KomfArchivePatch = {}
    if (d.archiveEnabled !== srcArchive.archiveEnabled) archive.enabled = d.archiveEnabled
    const hours = d.archiveUpdateIntervalHours.trim()
    if (hours !== srcArchive.archiveUpdateIntervalHours && nonNegativeInt(hours))
      archive.updateIntervalHours = Number(hours)
    const secs = d.archiveIdleReleaseSecs.trim()
    if (secs !== srcArchive.archiveIdleReleaseSecs && nonNegativeInt(secs)) archive.idleReleaseSecs = Number(secs)
    if (key === 'bangumi') {
      const dir = d.archiveDir.trim()
      if (dir !== srcArchive.archiveDir) archive.dir = dir === '' ? null : dir
      if (d.archiveStaffChineseNames !== srcArchive.archiveStaffChineseNames)
        archive.staffChineseNames = d.archiveStaffChineseNames
    } else {
      const url = d.archiveUrl.trim()
      if (url !== srcArchive.archiveUrl) archive.url = url === '' ? null : url
      const dbFile = d.archiveDbFile.trim()
      if (dbFile !== srcArchive.archiveDbFile) archive.dbFile = dbFile === '' ? null : dbFile
      if (!sameStringSet(cleanList(d.archiveSearchCategoryFilter), srcArchive.archiveSearchCategoryFilter))
        archive.searchCategoryFilter = cleanList(d.archiveSearchCategoryFilter)
      if (!sameStringSet(cleanList(d.archiveSearchUploaderFilter), srcArchive.archiveSearchUploaderFilter))
        archive.searchUploaderFilter = cleanList(d.archiveSearchUploaderFilter)
    }
    if (Object.keys(archive).length > 0) out.archive = archive
  }

  return out
}

function providersChanges(src: KomfProvidersMap, d: ProvidersDraft): ProvidersPatch {
  const out: ProvidersPatch = {}
  for (const key of PROVIDER_KEYS) {
    const sub = providerChanges(key, src[key], d[key])
    if (Object.keys(sub).length > 0) out[key] = sub
  }
  return out
}

/** A brand-new library override sends the whole draft (komf would otherwise seed its own defaults). */
function providerDraftToConfig(key: KomfProviderKey, d: ProviderDraft): KomfProviderConfig {
  const config: KomfProviderConfig = {
    enabled: d.enabled,
    priority: Number(d.priority),
    seriesMetadata: seriesMetadataDraftToConfig(d.seriesMetadata),
    bookMetadata: PROVIDERS_WITH_BOOKS.has(key) ? bookMetadataDraftToConfig(d.bookMetadata) : null,
    nameMatchingMode: d.nameMatchingMode === '' ? null : d.nameMatchingMode,
    mediaType: PROVIDERS_WITH_MEDIA_TYPE.has(key) ? d.mediaType : null,
    authorRoles: d.authorRoles,
    artistRoles: d.artistRoles,
  }
  if (key === 'aniList') {
    config.tagsScoreThreshold = Number(d.tagsScoreThreshold)
    config.tagsSizeLimit = Number(d.tagsSizeLimit)
    config.titleLanguagePriority = cleanList(d.titleLanguagePriority)
  }
  if (key === 'mangaDex' || key === 'mangaBaka') config.coverLanguages = cleanList(d.coverLanguages)
  if (key === 'mangaDex') config.links = d.links
  if (key === 'mangaBaka' || key === 'bookWalker') config.updateIntervalHours = Number(d.updateIntervalHours)
  if (key === 'mangaBaka') config.mode = d.mode
  if (key === 'bangumi') {
    config.tagWhitelist = cleanList(d.tagWhitelist)
    config.tagWhitelistFile = d.tagWhitelistFile.trim() === '' ? null : d.tagWhitelistFile.trim()
    config.archive = {
      enabled: d.archiveEnabled,
      dir: d.archiveDir.trim() === '' ? null : d.archiveDir.trim(),
      updateIntervalHours: Number(d.archiveUpdateIntervalHours),
      idleReleaseSecs: Number(d.archiveIdleReleaseSecs),
      staffChineseNames: d.archiveStaffChineseNames,
    }
  }
  if (key === 'eHentai') {
    config.preferredLanguages = cleanList(d.preferredLanguages)
    config.titlePriority = d.titlePriority
    config.translatorKeywords = cleanList(d.translatorKeywords)
    config.maleOnlyTagsFile = d.maleOnlyTagsFile.trim() === '' ? null : d.maleOnlyTagsFile.trim()
    config.titleTemplate = d.titleTemplate.trim()
    config.tagTranslationEnabled = d.tagTranslationEnabled
    config.tagTranslationUrl = d.tagTranslationUrl.trim()
    config.gidOnlyMatch = d.gidOnlyMatch
    config.searchDomain = d.searchDomain
    config.ipbMemberId = d.ipbMemberId.trim() === '' ? null : d.ipbMemberId.trim()
    config.ipbPassHash = d.ipbPassHash.trim() === '' ? null : d.ipbPassHash.trim()
    config.archive = {
      enabled: d.archiveEnabled,
      url: d.archiveUrl.trim() === '' ? null : d.archiveUrl.trim(),
      dbFile: d.archiveDbFile.trim() === '' ? null : d.archiveDbFile.trim(),
      updateIntervalHours: Number(d.archiveUpdateIntervalHours),
      idleReleaseSecs: Number(d.archiveIdleReleaseSecs),
      searchCategoryFilter: cleanList(d.archiveSearchCategoryFilter),
      searchUploaderFilter: cleanList(d.archiveSearchUploaderFilter),
    }
  }
  return config
}

function providersDraftToMap(d: ProvidersDraft): KomfProvidersMap {
  const out = {} as KomfProvidersMap
  for (const key of PROVIDER_KEYS) out[key] = providerDraftToConfig(key, d[key])
  return out
}

/** Index-merge patch for a masked URL list: deletions null out their index, additions append. */
function urlListChanges(
  src: string[] | null | undefined,
  entries: NotificationUrlEntry[],
): Record<number, string | null> | undefined {
  const base = src ?? []
  const patch: Record<number, string | null> = {}
  const kept = new Set(entries.filter((e) => e.key !== null).map((e) => e.key as number))
  for (let i = 0; i < base.length; i++) {
    if (!kept.has(i)) patch[i] = null
  }
  let append = 0
  for (const e of entries) {
    if (e.key === null && e.value.trim() !== '') patch[base.length + append++] = e.value.trim()
  }
  return Object.keys(patch).length > 0 ? patch : undefined
}

export function computeChanges(config: KomfConfig, d: KomfConfigDraft): KomfConfigPatch {
  const patch: KomfConfigPatch = {}

  const el = config.komga.eventListener
  const eventListener: Partial<KomfEventListenerConfig> = {}
  if (d.eventListenerEnabled !== el.enabled) eventListener.enabled = d.eventListenerEnabled
  if (!sameStringSet(d.eventListenerLibraryFilter, el.metadataLibraryFilter))
    eventListener.metadataLibraryFilter = d.eventListenerLibraryFilter
  if (!sameStringSet(cleanList(d.eventListenerSeriesExcludeFilter), el.metadataSeriesExcludeFilter ?? []))
    eventListener.metadataSeriesExcludeFilter = cleanList(d.eventListenerSeriesExcludeFilter)
  if (!sameStringSet(d.notificationsLibraryFilter, el.notificationsLibraryFilter ?? []))
    eventListener.notificationsLibraryFilter = d.notificationsLibraryFilter

  const defaultProcessing = processingChanges(config.komga.metadataUpdate.default, d.defaultProcessing)

  const library: Record<string, KomfProcessingPatch | null> = {}
  const processingIds = new Set([
    ...Object.keys(config.komga.metadataUpdate.library),
    ...Object.keys(d.libraryProcessing),
  ])
  for (const id of processingIds) {
    const had = config.komga.metadataUpdate.library[id]
    const now = d.libraryProcessing[id]
    if (now && had) {
      const sub = processingChanges(had, now)
      if (Object.keys(sub).length > 0) library[id] = sub
    } else if (now) {
      library[id] = processingDraftToPatch(now)
    } else if (had) {
      library[id] = null
    }
  }

  const metadataUpdate: NonNullable<NonNullable<KomfConfigPatch['komga']>['metadataUpdate']> = {}
  if (Object.keys(defaultProcessing).length > 0) metadataUpdate.default = defaultProcessing
  if (Object.keys(library).length > 0) metadataUpdate.library = library

  const komga: NonNullable<KomfConfigPatch['komga']> = {}
  if (Object.keys(eventListener).length > 0) komga.eventListener = eventListener
  if (Object.keys(metadataUpdate).length > 0) komga.metadataUpdate = metadataUpdate
  if (Object.keys(komga).length > 0) patch.komga = komga

  const n = d.notifications
  const srcN = config.notifications
  const notifications: NonNullable<KomfConfigPatch['notifications']> = {}
  const discord: NonNullable<NonNullable<KomfConfigPatch['notifications']>['discord']> = {}
  const webhooks = urlListChanges(srcN?.discord?.webhooks, n.discordWebhooks)
  if (webhooks) discord.webhooks = webhooks
  if (n.discordSeriesCover !== (srcN?.discord?.seriesCover ?? false)) discord.seriesCover = n.discordSeriesCover
  if (Object.keys(discord).length > 0) notifications.discord = discord
  const apprise: NonNullable<NonNullable<KomfConfigPatch['notifications']>['apprise']> = {}
  const urls = urlListChanges(srcN?.apprise?.urls, n.appriseUrls)
  if (urls) apprise.urls = urls
  if (n.appriseSeriesCover !== (srcN?.apprise?.seriesCover ?? false)) apprise.seriesCover = n.appriseSeriesCover
  if (Object.keys(apprise).length > 0) notifications.apprise = apprise
  if (Object.keys(notifications).length > 0) patch.notifications = notifications

  const mp = config.metadataProviders
  const providers: NonNullable<KomfConfigPatch['metadataProviders']> = {}
  if (d.nameMatchingMode !== mp.nameMatchingMode) providers.nameMatchingMode = d.nameMatchingMode
  const mal = d.malClientId.trim()
  if (mal !== (mp.malClientId ?? '')) providers.malClientId = mal === '' ? null : mal
  const cv = d.comicVineApiKey.trim()
  if (cv !== (mp.comicVineClientId ?? '')) providers.comicVineClientId = cv === '' ? null : cv
  const bgm = d.bangumiToken.trim()
  if (bgm !== (mp.bangumiToken ?? '')) providers.bangumiToken = bgm === '' ? null : bgm
  const cvs = d.comicVineSearchLimit.trim()
  if (cvs !== (mp.comicVineSearchLimit?.toString() ?? ''))
    providers.comicVineSearchLimit = cvs === '' ? null : Number(cvs)
  const cvi = d.comicVineIssueName.trim()
  if (cvi !== (mp.comicVineIssueName ?? '')) providers.comicVineIssueName = cvi === '' ? null : cvi
  if (d.comicVineIdFormat !== (mp.comicVineIdFormat ?? ''))
    providers.comicVineIdFormat = d.comicVineIdFormat === '' ? null : d.comicVineIdFormat

  const defaultProviders = providersChanges(mp.defaultProviders, d.defaultProviders)
  if (Object.keys(defaultProviders).length > 0) providers.defaultProviders = defaultProviders

  const libraryProviders: Record<string, ProvidersPatch | null> = {}
  const providerIds = new Set([...Object.keys(mp.libraryProviders), ...Object.keys(d.libraryProviders)])
  for (const id of providerIds) {
    const had = mp.libraryProviders[id]
    const now = d.libraryProviders[id]
    if (now && had) {
      const sub = providersChanges(had, now)
      if (Object.keys(sub).length > 0) libraryProviders[id] = sub
    } else if (now) {
      libraryProviders[id] = providersDraftToMap(now)
    } else if (had !== undefined && had !== null) {
      libraryProviders[id] = null
    }
  }
  if (Object.keys(libraryProviders).length > 0) providers.libraryProviders = libraryProviders
  if (Object.keys(providers).length > 0) patch.metadataProviders = providers

  return patch
}

/** Leaf-level change count for the save bar label. */
export function countChanges(patch: KomfConfigPatch): number {
  const count = (v: unknown): number => {
    if (v !== null && typeof v === 'object' && !Array.isArray(v))
      return Object.values(v).reduce((n, x) => n + count(x), 0)
    return 1
  }
  return count(patch)
}
