import type { Role } from '@/lib/api/types'

// USER is implicit on every account: the server always returns it, but it must
// never appear in submit payloads and is not worth a chip of its own.
// labelKey reuses the shared account role labels; descriptionKey is in admin-users.
export const ASSIGNABLE_ROLES: { value: Role; labelKey: string; descriptionKey: string }[] = [
  { value: 'ADMIN', labelKey: 'account:profile.role.admin', descriptionKey: 'roles.adminDesc' },
  { value: 'FILE_DOWNLOAD', labelKey: 'account:profile.role.fileDownload', descriptionKey: 'roles.fileDownloadDesc' },
  { value: 'PAGE_STREAMING', labelKey: 'account:profile.role.pageStreaming', descriptionKey: 'roles.pageStreamingDesc' },
  { value: 'KOBO_SYNC', labelKey: 'account:profile.role.koboSync', descriptionKey: 'roles.koboSyncDesc' },
  { value: 'KOREADER_SYNC', labelKey: 'account:profile.role.koreaderSync', descriptionKey: 'roles.koreaderSyncDesc' },
]
