import { api } from './client'
import type { ReleaseDto } from './types'

/** Admin-only GitHub releases proxy (cached server-side). */
export const releasesApi = {
  list: () => api.get<ReleaseDto[]>('/api/v1/releases'),
}
