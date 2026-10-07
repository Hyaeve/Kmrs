import type { PageHashAction } from '@/lib/api/types'

export const ACTION_LABEL_KEYS: Record<PageHashAction, string> = {
  DELETE_AUTO: 'admin-maintenance:pageHashes.action.deleteAuto',
  DELETE_MANUAL: 'admin-maintenance:pageHashes.action.deleteManual',
  IGNORE: 'admin-maintenance:pageHashes.action.ignore',
}
