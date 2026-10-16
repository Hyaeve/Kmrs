import { api, pageQuery } from './client'
import type {
  Page,
  PageHashAction,
  PageHashCreationDto,
  PageHashKnownDto,
  PageHashMatchDto,
  PageHashUnknownDto,
  PageParams,
} from './types'

/** Admin-only duplicate page-hash management. */
export const pageHashesApi = {
  listKnown: (params?: PageParams & { action?: PageHashAction[] }) =>
    api.get<Page<PageHashKnownDto>>('/api/v1/page-hashes', { ...pageQuery(params), action: params?.action }),

  createOrUpdate: (body: PageHashCreationDto) => api.put<void>('/api/v1/page-hashes', body),

  listUnknown: (params?: PageParams) => api.get<Page<PageHashUnknownDto>>('/api/v1/page-hashes/unknown', pageQuery(params)),

  matches: (hash: string, params?: PageParams) =>
    api.get<Page<PageHashMatchDto>>(`/api/v1/page-hashes/${hash}`, pageQuery(params)),

  deleteAll: (hash: string) => api.post<void>(`/api/v1/page-hashes/${hash}/delete-all`),

  deleteMatch: (hash: string, match: PageHashMatchDto) =>
    api.post<void>(`/api/v1/page-hashes/${hash}/delete-match`, match),

  knownThumbnailUrl: (hash: string) => `/api/v1/page-hashes/${hash}/thumbnail`,
  unknownThumbnailUrl: (hash: string, resize?: number) =>
    `/api/v1/page-hashes/unknown/${hash}/thumbnail${resize ? `?resize=${resize}` : ''}`,
}
