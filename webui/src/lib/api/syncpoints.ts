import { api } from './client'

export const syncpointsApi = {
  /** Without keyIds every sync point of the current user is deleted. */
  deleteMine: (keyIds?: string[]) => api.delete<void>('/api/v1/syncpoints/me', undefined, { key_id: keyIds }),
}
