import { api } from './client'
import type { LibraryCreationDto, LibraryDto, LibraryUpdateDto } from './types'

export const librariesApi = {
  list: () => api.get<LibraryDto[]>('/api/v1/libraries'),
  get: (libraryId: string) => api.get<LibraryDto>(`/api/v1/libraries/${libraryId}`),

  // Admin operations
  create: (body: LibraryCreationDto) => api.post<LibraryDto>('/api/v1/libraries', body),
  update: (libraryId: string, body: LibraryUpdateDto) => api.patch<void>(`/api/v1/libraries/${libraryId}`, body),
  delete: (libraryId: string) => api.delete<void>(`/api/v1/libraries/${libraryId}`),
  scan: (libraryId: string, deep = false) => api.post<void>(`/api/v1/libraries/${libraryId}/scan`, undefined, { deep }),
  analyze: (libraryId: string) => api.post<void>(`/api/v1/libraries/${libraryId}/analyze`),
  refreshMetadata: (libraryId: string) => api.post<void>(`/api/v1/libraries/${libraryId}/metadata/refresh`),
  emptyTrash: (libraryId: string) => api.post<void>(`/api/v1/libraries/${libraryId}/empty-trash`),
}
