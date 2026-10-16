// kmrs-private smart lists: user-owned search filters evaluated live by the server.
// items endpoints take the page-side filters as an overlay body (ANDed with the stored filter).
import { api, pageQuery } from './client'
import type {
  BookDto,
  BookSearch,
  Page,
  PageParams,
  SeriesDto,
  SeriesSearch,
  ShareTargetDto,
  SmartListCreationDto,
  SmartListDto,
  SmartListUpdateDto,
} from './types'

export const smartListsApi = {
  list: (params?: PageParams & { owner?: string }) =>
    api.get<Page<SmartListDto>>('/api/v1/smart-lists', { ...pageQuery(params), owner: params?.owner }),
  /** minimal user directory (id, email) for picking share targets */
  shareTargets: () => api.get<ShareTargetDto[]>('/api/v1/smart-lists/share-targets'),
  get: (smartListId: string) => api.get<SmartListDto>(`/api/v1/smart-lists/${smartListId}`),
  books: (smartListId: string, params?: PageParams, overlay?: BookSearch) =>
    api.post<Page<BookDto>>(`/api/v1/smart-lists/${smartListId}/books`, overlay ?? {}, pageQuery(params)),
  series: (smartListId: string, params?: PageParams, overlay?: SeriesSearch) =>
    api.post<Page<SeriesDto>>(`/api/v1/smart-lists/${smartListId}/series`, overlay ?? {}, pageQuery(params)),
  create: (body: SmartListCreationDto) => api.post<SmartListDto>('/api/v1/smart-lists', body),
  update: (smartListId: string, body: SmartListUpdateDto) =>
    api.patch<void>(`/api/v1/smart-lists/${smartListId}`, body),
  delete: (smartListId: string) => api.delete<void>(`/api/v1/smart-lists/${smartListId}`),
}
