import { api, ApiError } from './client'
import type { LatestReleaseDto, ReleaseDto } from './types'

/** Admin-only GitHub releases proxy (cached server-side). */
export const releasesApi = {
  list: () => api.get<ReleaseDto[]>('/api/v1/releases'),
  latest: () =>
    api.get<LatestReleaseDto>('/api/v1/releases/latest').catch((e) => {
      // 404 is the routine "no newer release" answer, not a failure
      if (e instanceof ApiError && e.status === 404) return null
      throw e
    }),
}
