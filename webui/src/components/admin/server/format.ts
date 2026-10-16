import i18n from '@/lib/i18n'

/** Compact duration: "3d 4h" / "4h 12m" / "5m" / "42s", sub-second as ms. */
export function formatDuration(seconds: number): string {
  if (seconds < 1) return i18n.t('admin-settings:duration.ms', { count: Math.round(seconds * 1000) })
  if (seconds < 60) return i18n.t('admin-settings:duration.second', { count: Math.floor(seconds) })
  const d = Math.floor(seconds / 86400)
  const h = Math.floor((seconds % 86400) / 3600)
  const m = Math.floor((seconds % 3600) / 60)
  const parts: string[] = []
  if (d > 0) parts.push(i18n.t('admin-settings:duration.day', { count: d }))
  if (h > 0) parts.push(i18n.t('admin-settings:duration.hour', { count: h }))
  if (d === 0 && m > 0) parts.push(i18n.t('admin-settings:duration.minute', { count: m }))
  return parts.slice(0, 2).join(' ')
}

export function formatInterval(ms: number): string {
  return i18n.t('admin-settings:interval', { duration: formatDuration(ms / 1000) })
}

const TASK_TYPE_KEYS: Record<string, string> = {
  ScanLibrary: 'scanLibrary',
  FindBooksToConvert: 'findBooksToConvert',
  FindBooksWithMissingPageHash: 'findBooksWithMissingPageHash',
  FindDuplicatePagesToDelete: 'findDuplicatePagesToDelete',
  EmptyTrash: 'emptyTrash',
  AnalyzeBook: 'analyzeBook',
  GenerateBookThumbnail: 'generateBookThumbnail',
  RefreshBookMetadata: 'refreshBookMetadata',
  HashBook: 'hashBook',
  HashBookPages: 'hashBookPages',
  HashBookKoreader: 'hashBookKoreader',
  RefreshSeriesMetadata: 'refreshSeriesMetadata',
  AggregateSeriesMetadata: 'aggregateSeriesMetadata',
  RefreshBookLocalArtwork: 'refreshBookLocalArtwork',
  RefreshSeriesLocalArtwork: 'refreshSeriesLocalArtwork',
  ImportBook: 'importBook',
  ConvertBook: 'convertBook',
  RepairExtension: 'repairExtension',
  RemoveHashedPages: 'removeHashedPages',
  RebuildIndex: 'rebuildIndex',
  UpgradeIndex: 'upgradeIndex',
  DeleteBook: 'deleteBook',
  DeleteSeries: 'deleteSeries',
  FindBookThumbnailsToRegenerate: 'findBookThumbnailsToRegenerate',
}

/** Task types from a newer server than this web UI fall back to a camelCase split. */
export function taskTypeLabel(type: string): string {
  const key = TASK_TYPE_KEYS[type]
  if (key) return i18n.t(`admin-settings:taskType.${key}`)
  const words = type.replace(/([a-z0-9])([A-Z])/g, '$1 $2')
  return words.charAt(0).toUpperCase() + words.slice(1).toLowerCase()
}

/** Drop package prefixes / lambda suffixes so "a.b.SseController.heartbeat" → "SseController.heartbeat". */
export function targetLabel(target: string): string {
  const cleaned = target.replace(/\$.*$/, '')
  const parts = cleaned.split('.')
  return parts.length >= 3 ? parts.slice(-2).join('.') : cleaned
}
