import { api } from './client'
import type { ReleaseDto, UpdateCheckDto } from './types'

/** Admin-only GitHub releases proxy (cached server-side). */
export const releasesApi = {
  list: () => api.get<ReleaseDto[]>('/api/v1/releases'),
  updateCheck: () => api.get<UpdateCheckDto>('/api/v1/update-check'),
}
