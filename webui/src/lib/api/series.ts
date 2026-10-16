import { api, pageQuery } from './client'
import type {
  BookDto,
  CollectionDto,
  GroupCountDto,
  Page,
  PageParams,
  SeriesDto,
  SeriesMetadataUpdateDto,
  SeriesSearch,
} from './types'

export interface SeriesListQuery extends PageParams {
  search?: SeriesSearch
}

export const seriesApi = {
  list: (query: SeriesListQuery) =>
    api.post<Page<SeriesDto>>('/api/v1/series/list', query.search ?? {}, pageQuery(query)),

  latest: (params?: PageParams & { libraryId?: string[]; oneshot?: boolean }) =>
    api.get<Page<SeriesDto>>('/api/v1/series/latest', {
      ...pageQuery(params),
      library_id: params?.libraryId,
      oneshot: params?.oneshot,
    }),

  new: (params?: PageParams & { libraryId?: string[]; oneshot?: boolean }) =>
    api.get<Page<SeriesDto>>('/api/v1/series/new', {
      ...pageQuery(params),
      library_id: params?.libraryId,
      oneshot: params?.oneshot,
    }),

  updated: (params?: PageParams & { libraryId?: string[]; oneshot?: boolean }) =>
    api.get<Page<SeriesDto>>('/api/v1/series/updated', {
      ...pageQuery(params),
      library_id: params?.libraryId,
      oneshot: params?.oneshot,
    }),

  get: (seriesId: string) => api.get<SeriesDto>(`/api/v1/series/${seriesId}`),

  books: (seriesId: string, params?: PageParams & { readStatus?: string[]; mediaStatus?: string[] }) =>
    api.get<Page<BookDto>>(`/api/v1/series/${seriesId}/books`, {
      ...pageQuery(params),
      read_status: params?.readStatus,
      media_status: params?.mediaStatus,
    }),

  collections: (seriesId: string) => api.get<CollectionDto[]>(`/api/v1/series/${seriesId}/collections`),

  alphabeticalGroups: (search?: SeriesSearch) =>
    api.post<GroupCountDto[]>('/api/v1/series/list/alphabetical-groups', search ?? {}),

  markRead: (seriesId: string) => api.post<void>(`/api/v1/series/${seriesId}/read-progress`),
  markUnread: (seriesId: string) => api.delete<void>(`/api/v1/series/${seriesId}/read-progress`),

  // Admin operations
  patchMetadata: (seriesId: string, body: SeriesMetadataUpdateDto) =>
    api.patch<void>(`/api/v1/series/${seriesId}/metadata`, body),
  deleteFile: (seriesId: string) => api.delete<void>(`/api/v1/series/${seriesId}/file`),
  analyze: (seriesId: string) => api.post<void>(`/api/v1/series/${seriesId}/analyze`),
  refreshMetadata: (seriesId: string) => api.post<void>(`/api/v1/series/${seriesId}/metadata/refresh`),
}
