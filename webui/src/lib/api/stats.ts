import { api } from './client'
import type { LibrariesStatsDto, ReadingActivityDto, ReadingSummaryDto, ReadingTopsDto, ServerStatsDto } from './types'

/** kmrs-private statistics; the reading endpoints take an optional library filter. */
export const statsApi = {
  summary: (libraryId?: string) => api.get<ReadingSummaryDto>('/api/v1/stats/reading/summary', { libraryId }),
  activity: (libraryId: string | undefined, tzOffsetMinutes: number) =>
    api.get<ReadingActivityDto>('/api/v1/stats/reading/activity', { libraryId, tzOffsetMinutes }),
  tops: (libraryId?: string) => api.get<ReadingTopsDto>('/api/v1/stats/reading/tops', { libraryId }),
  /** Content counts per visible library, empty libraries included with zeros. */
  libraries: () => api.get<LibrariesStatsDto>('/api/v1/stats/libraries'),
  /** Admin only. */
  server: () => api.get<ServerStatsDto>('/api/v1/stats/server'),
}
