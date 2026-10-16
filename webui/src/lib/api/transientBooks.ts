import { api } from './client'
import type { TransientBookDto } from './types'

/** Admin-only scan/analyze of books outside any library, ahead of import. */
export const transientBooksApi = {
  scan: (path: string) => api.post<TransientBookDto[]>('/api/v1/transient-books', { path }),
  analyze: (id: string) => api.post<TransientBookDto>(`/api/v1/transient-books/${id}/analyze`),
  pageUrl: (id: string, pageNumber: number) => `/api/v1/transient-books/${id}/pages/${pageNumber}`,
}
