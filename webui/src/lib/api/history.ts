import { api, pageQuery } from './client'
import type { HistoricalEventDto, Page, PageParams } from './types'

/** Admin-only historical events; the server defaults to timestamp descending. */
export const historyApi = {
  list: (params?: PageParams) => api.get<Page<HistoricalEventDto>>('/api/v1/history', pageQuery(params)),
}
