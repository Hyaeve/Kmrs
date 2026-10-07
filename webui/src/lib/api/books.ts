import { api, pageQuery } from './client'
import type {
  BookDto,
  BookImportBatchDto,
  BookMetadataUpdateDto,
  BookSearch,
  Page,
  PageDto,
  PageParams,
  R2PositionsDto,
  R2ProgressionDto,
  ReadListDto,
} from './types'

export interface BookListQuery extends PageParams {
  search?: BookSearch
}

export const booksApi = {
  list: (query: BookListQuery) =>
    api.post<Page<BookDto>>('/api/v1/books/list', query.search ?? {}, pageQuery(query)),

  latest: (params?: PageParams) => api.get<Page<BookDto>>('/api/v1/books/latest', pageQuery(params)),

  ondeck: (params?: PageParams & { libraryId?: string[] }) =>
    api.get<Page<BookDto>>('/api/v1/books/ondeck', { ...pageQuery(params), library_id: params?.libraryId }),

  get: (bookId: string) => api.get<BookDto>(`/api/v1/books/${bookId}`),

  previous: (bookId: string) => api.get<BookDto>(`/api/v1/books/${bookId}/previous`),
  next: (bookId: string) => api.get<BookDto>(`/api/v1/books/${bookId}/next`),

  pages: (bookId: string) => api.get<PageDto[]>(`/api/v1/books/${bookId}/pages`),

  readlists: (bookId: string) => api.get<ReadListDto[]>(`/api/v1/books/${bookId}/readlists`),

  positions: (bookId: string) => api.get<R2PositionsDto>(`/api/v1/books/${bookId}/positions`),
  /** undefined when the user has no stored progression (204) */
  progression: (bookId: string) => api.get<R2ProgressionDto | undefined>(`/api/v1/books/${bookId}/progression`),
  updateProgression: (bookId: string, body: R2ProgressionDto) =>
    api.put<void>(`/api/v1/books/${bookId}/progression`, body),

  updateProgress: (bookId: string, progress: { page?: number; completed?: boolean }) =>
    api.patch<void>(`/api/v1/books/${bookId}/read-progress`, progress),

  markUnread: (bookId: string) => api.delete<void>(`/api/v1/books/${bookId}/read-progress`),
  markRead: (bookId: string) => api.patch<void>(`/api/v1/books/${bookId}/read-progress`, { completed: true }),

  // Admin operations
  patchMetadata: (bookId: string, body: BookMetadataUpdateDto) =>
    api.patch<void>(`/api/v1/books/${bookId}/metadata`, body),
  /** Batch metadata patch: maps book id to its update. */
  bulkPatchMetadata: (body: Record<string, BookMetadataUpdateDto>) =>
    api.patch<void>('/api/v1/books/metadata', body),
  deleteFile: (bookId: string) => api.delete<void>(`/api/v1/books/${bookId}/file`),
  analyze: (bookId: string) => api.post<void>(`/api/v1/books/${bookId}/analyze`),
  refreshMetadata: (bookId: string) => api.post<void>(`/api/v1/books/${bookId}/metadata/refresh`),
  duplicates: (params?: PageParams) => api.get<Page<BookDto>>('/api/v1/books/duplicates', pageQuery(params)),
  importBooks: (body: BookImportBatchDto) => api.post<void>('/api/v1/books/import', body),
  /** forBiggerResultOnly skips books whose current thumbnail already meets the configured size. */
  regenerateThumbnails: (forBiggerResultOnly: boolean) =>
    api.put<void>('/api/v1/books/thumbnails', undefined, { for_bigger_result_only: forBiggerResultOnly }),
}
