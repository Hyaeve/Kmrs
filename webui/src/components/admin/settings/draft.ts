import i18n from '@/lib/i18n'
import type { SettingsDto, SettingsUpdateDto, ThumbnailSize } from '@/lib/api/types'

/** Editable form mirror of SettingsDto: inputs are strings, '' means "no value / no override". */
export interface SettingsDraft {
  deleteEmptyCollections: boolean
  deleteEmptyReadLists: boolean
  rememberMeDurationDays: string
  thumbnailSize: ThumbnailSize
  taskPoolSize: string
  serverPort: string
  serverContextPath: string
  koboProxy: boolean
  koboPort: string
  kepubifyPath: string
}

export function draftFromSettings(s: SettingsDto): SettingsDraft {
  return {
    deleteEmptyCollections: s.deleteEmptyCollections ?? false,
    deleteEmptyReadLists: s.deleteEmptyReadLists ?? false,
    rememberMeDurationDays: s.rememberMeDurationDays?.toString() ?? '',
    thumbnailSize: s.thumbnailSize ?? 'DEFAULT',
    taskPoolSize: s.taskPoolSize?.toString() ?? '',
    // multi-source fields edit the database override only; config-file values are read-only here
    serverPort: s.serverPort?.databaseSource?.toString() ?? '',
    serverContextPath: s.serverContextPath?.databaseSource ?? '',
    koboProxy: s.koboProxy ?? false,
    koboPort: s.koboPort?.toString() ?? '',
    kepubifyPath: s.kepubifyPath?.databaseSource ?? '',
  }
}

const CONTEXT_PATH_RE = /^\/[\w-/]*[a-zA-Z0-9]$/

function positiveInt(v: string): boolean {
  return /^\d+$/.test(v) && Number(v) > 0
}

export function validateDraft(d: SettingsDraft): Partial<Record<keyof SettingsDraft, string>> {
  const errors: Partial<Record<keyof SettingsDraft, string>> = {}
  if (!positiveInt(d.rememberMeDurationDays.trim()))
    errors.rememberMeDurationDays = i18n.t('admin-settings:validation.positiveDays')
  if (!positiveInt(d.taskPoolSize.trim())) errors.taskPoolSize = i18n.t('admin-settings:validation.positiveInt')
  if (d.serverPort.trim() && !positiveInt(d.serverPort.trim()))
    errors.serverPort = i18n.t('admin-settings:validation.positiveInt')
  if (d.serverContextPath.trim() && !CONTEXT_PATH_RE.test(d.serverContextPath.trim()))
    errors.serverContextPath = i18n.t('admin-settings:validation.contextPath')
  if (d.koboPort.trim() && !positiveInt(d.koboPort.trim())) errors.koboPort = i18n.t('admin-settings:validation.positiveInt')
  return errors
}

/** PATCH semantics: only dirty fields, null clears a database override. */
export function computeChanges(s: SettingsDto, d: SettingsDraft): SettingsUpdateDto {
  const changes: SettingsUpdateDto = {}
  if (d.deleteEmptyCollections !== (s.deleteEmptyCollections ?? false)) changes.deleteEmptyCollections = d.deleteEmptyCollections
  if (d.deleteEmptyReadLists !== (s.deleteEmptyReadLists ?? false)) changes.deleteEmptyReadLists = d.deleteEmptyReadLists
  if (d.rememberMeDurationDays.trim() !== (s.rememberMeDurationDays?.toString() ?? ''))
    changes.rememberMeDurationDays = Number(d.rememberMeDurationDays)
  if (d.thumbnailSize !== (s.thumbnailSize ?? 'DEFAULT')) changes.thumbnailSize = d.thumbnailSize
  if (d.taskPoolSize.trim() !== (s.taskPoolSize?.toString() ?? '')) changes.taskPoolSize = Number(d.taskPoolSize)
  const port = d.serverPort.trim()
  if (port !== (s.serverPort?.databaseSource?.toString() ?? '')) changes.serverPort = port === '' ? null : Number(port)
  const ctx = d.serverContextPath.trim()
  if (ctx !== (s.serverContextPath?.databaseSource ?? '')) changes.serverContextPath = ctx === '' ? null : ctx
  if (d.koboProxy !== (s.koboProxy ?? false)) changes.koboProxy = d.koboProxy
  const kobo = d.koboPort.trim()
  if (kobo !== (s.koboPort?.toString() ?? '')) changes.koboPort = kobo === '' ? null : Number(kobo)
  const kep = d.kepubifyPath.trim()
  if (kep !== (s.kepubifyPath?.databaseSource ?? '')) changes.kepubifyPath = kep === '' ? null : kep
  return changes
}
