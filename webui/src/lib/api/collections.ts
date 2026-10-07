import { api, pageQuery } from './client'
import type { BookDto, CollectionDto, Page, PageParams, ReadListDto, ReadListRequestMatchDto, SeriesDto } from './types'

export const collectionsApi = {
  list: (params?: PageParams & { search?: string; libraryId?: string[] }) =>
    api.get<Page<CollectionDto>>('/api/v1/collections', {
      ...pageQuery(params),
      search: params?.search,
      library_id: params?.libraryId,
    }),
  get: (collectionId: string) => api.get<CollectionDto>(`/api/v1/collections/${collectionId}`),
  series: (collectionId: string, params?: PageParams & { readStatus?: string[] }) =>
    api.get<Page<SeriesDto>>(`/api/v1/collections/${collectionId}/series`, {
      ...pageQuery(params),
      read_status: params?.readStatus,
    }),
  create: (body: { name: string; ordered: boolean; seriesIds: string[] }) =>
    api.post<CollectionDto>('/api/v1/collections', body),
  update: (collectionId: string, body: Partial<{ name: string; ordered: boolean; seriesIds: string[] }>) =>
    api.patch<void>(`/api/v1/collections/${collectionId}`, body),
  delete: (collectionId: string) => api.delete<void>(`/api/v1/collections/${collectionId}`),
}

export const readlistsApi = {
  list: (params?: PageParams & { search?: string; libraryId?: string[] }) =>
    api.get<Page<ReadListDto>>('/api/v1/readlists', {
      ...pageQuery(params),
      search: params?.search,
      library_id: params?.libraryId,
    }),
  get: (readListId: string) => api.get<ReadListDto>(`/api/v1/readlists/${readListId}`),
  books: (readListId: string, params?: PageParams & { readStatus?: string[] }) =>
    api.get<Page<BookDto>>(`/api/v1/readlists/${readListId}/books`, {
      ...pageQuery(params),
      read_status: params?.readStatus,
    }),
  previous: (readListId: string, bookId: string) =>
    api.get<BookDto>(`/api/v1/readlists/${readListId}/books/${bookId}/previous`),
  next: (readListId: string, bookId: string) =>
    api.get<BookDto>(`/api/v1/readlists/${readListId}/books/${bookId}/next`),
  create: (body: { name: string; summary: string; ordered: boolean; bookIds: string[] }) =>
    api.post<ReadListDto>('/api/v1/readlists', body),
  update: (readListId: string, body: Partial<{ name: string; summary: string; ordered: boolean; bookIds: string[] }>) =>
    api.patch<void>(`/api/v1/readlists/${readListId}`, body),
  delete: (readListId: string) => api.delete<void>(`/api/v1/readlists/${readListId}`),

  /** Matches a ComicRack ReadingList XML file (multipart `file` part) against the library. */
  matchComicRack: (file: Blob) => {
    const form = new FormData()
    form.append('file', file)
    return api.upload<ReadListRequestMatchDto>('/api/v1/readlists/match/comicrack', form)
  },
}
