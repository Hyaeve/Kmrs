import { ApiError } from '@/lib/api/client'
import type { LibraryCreationDto, LibraryDto, LibraryUpdateDto, ScanInterval, SeriesCover } from '@/lib/api/types'

/** Every editable boolean flag on a library (unavailable is server-managed). */
export type BoolKey = Exclude<
  { [K in keyof LibraryDto]-?: LibraryDto[K] extends boolean ? K : never }[keyof LibraryDto],
  'unavailable'
>

export type LibraryFormState = Pick<LibraryDto, BoolKey | 'scanInterval' | 'seriesCover' | 'scanDirectoryExclusions'> & {
  name: string
  root: string
  /** '' = unset; mapped to omitted on create, null on update when cleared */
  oneshotsDirectory: string
}

const BOOL_KEYS: BoolKey[] = [
  'importComicInfoBook',
  'importComicInfoSeries',
  'importComicInfoCollection',
  'importComicInfoReadList',
  'importComicInfoSeriesAppendVolume',
  'importEpubBook',
  'importEpubSeries',
  'importMylarSeries',
  'importLocalArtwork',
  'importBarcodeIsbn',
  'scanForceModifiedTime',
  'scanOnStartup',
  'scanCbx',
  'scanPdf',
  'scanEpub',
  'repairExtensions',
  'convertToCbz',
  'emptyTrashAfterScan',
  'hashFiles',
  'hashPages',
  'hashKoreader',
  'analyzeDimensions',
]

/** Server-side defaults; create bodies diff against this so unchanged fields stay omitted. */
export const DEFAULTS: LibraryFormState = {
  name: '',
  root: '',
  oneshotsDirectory: '',
  scanInterval: 'EVERY_6H',
  scanOnStartup: false,
  scanForceModifiedTime: false,
  scanCbx: true,
  scanPdf: true,
  scanEpub: true,
  scanDirectoryExclusions: [],
  importComicInfoBook: true,
  importComicInfoSeries: true,
  importComicInfoCollection: true,
  importComicInfoReadList: true,
  importComicInfoSeriesAppendVolume: true,
  importEpubBook: true,
  importEpubSeries: true,
  importMylarSeries: true,
  importLocalArtwork: true,
  importBarcodeIsbn: true,
  hashFiles: true,
  hashPages: false,
  hashKoreader: false,
  analyzeDimensions: true,
  repairExtensions: false,
  convertToCbz: false,
  emptyTrashAfterScan: false,
  seriesCover: 'FIRST',
}

export function formFromLibrary(lib: LibraryDto): LibraryFormState {
  const bools = {} as Pick<LibraryFormState, BoolKey>
  for (const k of BOOL_KEYS) bools[k] = lib[k]
  return {
    ...bools,
    name: lib.name,
    root: lib.root,
    oneshotsDirectory: lib.oneshotsDirectory ?? '',
    scanInterval: lib.scanInterval,
    seriesCover: lib.seriesCover,
    scanDirectoryExclusions: [...lib.scanDirectoryExclusions],
  }
}

function sameStrings(a: string[], b: string[]): boolean {
  return a.length === b.length && a.every((v, i) => v === b[i])
}

export function diffCreation(s: LibraryFormState): LibraryCreationDto {
  const body: LibraryCreationDto = { name: s.name.trim(), root: s.root.trim() }
  const oneshots = s.oneshotsDirectory.trim()
  if (oneshots) body.oneshotsDirectory = oneshots
  for (const k of BOOL_KEYS) if (s[k] !== DEFAULTS[k]) body[k] = s[k]
  if (s.scanInterval !== DEFAULTS.scanInterval) body.scanInterval = s.scanInterval
  if (s.seriesCover !== DEFAULTS.seriesCover) body.seriesCover = s.seriesCover
  if (s.scanDirectoryExclusions.length) body.scanDirectoryExclusions = s.scanDirectoryExclusions
  return body
}

export function diffUpdate(s: LibraryFormState, orig: LibraryDto): LibraryUpdateDto {
  const body: LibraryUpdateDto = {}
  const name = s.name.trim()
  const root = s.root.trim()
  if (name !== orig.name) body.name = name
  if (root !== orig.root) body.root = root
  const oneshots = s.oneshotsDirectory.trim()
  if (oneshots !== (orig.oneshotsDirectory ?? '')) body.oneshotsDirectory = oneshots || null
  for (const k of BOOL_KEYS) if (s[k] !== orig[k]) body[k] = s[k]
  if (s.scanInterval !== orig.scanInterval) body.scanInterval = s.scanInterval
  if (s.seriesCover !== orig.seriesCover) body.seriesCover = s.seriesCover
  if (!sameStrings(s.scanDirectoryExclusions, orig.scanDirectoryExclusions))
    body.scanDirectoryExclusions = s.scanDirectoryExclusions
  return body
}

/** Editing any of these makes the server rescan the library. */
export const RESCAN_PATCH_KEYS = [
  'root',
  'oneshotsDirectory',
  'scanCbx',
  'scanPdf',
  'scanEpub',
  'scanForceModifiedTime',
  'scanDirectoryExclusions',
] as const

/** Newly enabling any of these queues background tasks for existing books. */
export const BATCH_ENABLE_KEYS = ['hashFiles', 'hashPages', 'hashKoreader', 'repairExtensions', 'convertToCbz'] as const

export const SCAN_INTERVAL_OPTIONS: { value: ScanInterval; labelKey: string }[] = [
  { value: 'DISABLED', labelKey: 'admin-maintenance:libraries.scanInterval.disabled' },
  { value: 'HOURLY', labelKey: 'admin-maintenance:libraries.scanInterval.hourly' },
  { value: 'EVERY_6H', labelKey: 'admin-maintenance:libraries.scanInterval.every6h' },
  { value: 'EVERY_12H', labelKey: 'admin-maintenance:libraries.scanInterval.every12h' },
  { value: 'DAILY', labelKey: 'admin-maintenance:libraries.scanInterval.daily' },
  { value: 'WEEKLY', labelKey: 'admin-maintenance:libraries.scanInterval.weekly' },
]

export const SERIES_COVER_OPTIONS: { value: SeriesCover; labelKey: string }[] = [
  { value: 'FIRST', labelKey: 'admin-maintenance:libraries.seriesCoverOption.first' },
  { value: 'FIRST_UNREAD_OR_FIRST', labelKey: 'admin-maintenance:libraries.seriesCoverOption.firstUnreadOrFirst' },
  { value: 'FIRST_UNREAD_OR_LAST', labelKey: 'admin-maintenance:libraries.seriesCoverOption.firstUnreadOrLast' },
  { value: 'LAST', labelKey: 'admin-maintenance:libraries.seriesCoverOption.last' },
]

export function scanIntervalBadgeKey(interval: ScanInterval): string | null {
  switch (interval) {
    case 'DISABLED':
      return null
    case 'HOURLY':
      return 'admin-maintenance:libraries.scanInterval.hourly'
    case 'EVERY_6H':
      return 'admin-maintenance:libraries.scanInterval.every6h'
    case 'EVERY_12H':
      return 'admin-maintenance:libraries.scanInterval.every12h'
    case 'DAILY':
      return 'admin-maintenance:libraries.scanInterval.daily'
    case 'WEEKLY':
      return 'admin-maintenance:libraries.scanInterval.weekly'
  }
}

export const SCAN_SWITCH_FIELDS: { key: BoolKey; labelKey: string }[] = [
  { key: 'scanOnStartup', labelKey: 'admin-maintenance:libraries.scan.onStartup' },
  { key: 'scanForceModifiedTime', labelKey: 'admin-maintenance:libraries.scan.forceModifiedTime' },
  { key: 'scanCbx', labelKey: 'admin-maintenance:libraries.scan.cbx' },
  { key: 'scanPdf', labelKey: 'admin-maintenance:libraries.scan.pdf' },
  { key: 'scanEpub', labelKey: 'admin-maintenance:libraries.scan.epub' },
]

export const IMPORT_FIELDS: { key: BoolKey; labelKey: string }[] = [
  { key: 'importComicInfoBook', labelKey: 'admin-maintenance:libraries.import.comicInfoBook' },
  { key: 'importComicInfoSeries', labelKey: 'admin-maintenance:libraries.import.comicInfoSeries' },
  { key: 'importComicInfoCollection', labelKey: 'admin-maintenance:libraries.import.comicInfoCollection' },
  { key: 'importComicInfoReadList', labelKey: 'admin-maintenance:libraries.import.comicInfoReadList' },
  { key: 'importComicInfoSeriesAppendVolume', labelKey: 'admin-maintenance:libraries.import.comicInfoSeriesAppendVolume' },
  { key: 'importEpubBook', labelKey: 'admin-maintenance:libraries.import.epubBook' },
  { key: 'importEpubSeries', labelKey: 'admin-maintenance:libraries.import.epubSeries' },
  { key: 'importMylarSeries', labelKey: 'admin-maintenance:libraries.import.mylarSeries' },
  { key: 'importLocalArtwork', labelKey: 'admin-maintenance:libraries.import.localArtwork' },
  { key: 'importBarcodeIsbn', labelKey: 'admin-maintenance:libraries.import.barcodeIsbn' },
]

export const ANALYSIS_FIELDS: { key: BoolKey; labelKey: string }[] = [
  { key: 'hashFiles', labelKey: 'admin-maintenance:libraries.analysis.hashFiles' },
  { key: 'hashPages', labelKey: 'admin-maintenance:libraries.analysis.hashPages' },
  { key: 'hashKoreader', labelKey: 'admin-maintenance:libraries.analysis.hashKoreader' },
  { key: 'analyzeDimensions', labelKey: 'admin-maintenance:libraries.analysis.analyzeDimensions' },
  { key: 'repairExtensions', labelKey: 'admin-maintenance:libraries.analysis.repairExtensions' },
  { key: 'convertToCbz', labelKey: 'admin-maintenance:libraries.analysis.convertToCbz' },
  { key: 'emptyTrashAfterScan', labelKey: 'admin-maintenance:libraries.analysis.emptyTrashAfterScan' },
]

export interface Violation {
  fieldName: string
  message: string
}

/** Spring 400 bodies carry {violations:[{fieldName,message}]}; anything else yields []. */
export function parseViolations(err: unknown): Violation[] {
  if (!(err instanceof ApiError)) return []
  const body = err.body
  if (!body || typeof body !== 'object' || !('violations' in body)) return []
  const list = (body as { violations: unknown }).violations
  if (!Array.isArray(list)) return []
  return list.filter(
    (v): v is Violation =>
      !!v &&
      typeof v === 'object' &&
      typeof (v as Violation).fieldName === 'string' &&
      typeof (v as Violation).message === 'string',
  )
}
