import { api } from './client'
import type {
  KomfConfig,
  KomfConfigPatch,
  KomfIdentifyRequest,
  KomfIntegrationDto,
  KomfIntegrationUpdateDto,
  KomfJob,
  KomfMetadataJobResponse,
  KomfOAuthProvider,
  KomfOAuthStatus,
  KomfSeriesSearchResult,
} from './types'

export const komfApi = {
  getIntegration: () => api.get<KomfIntegrationDto>('/api/v1/komf/integration'),
  updateIntegration: (body: KomfIntegrationUpdateDto) => api.put<KomfIntegrationDto>('/api/v1/komf/integration', body),
  disconnect: () => api.delete<void>('/api/v1/komf/integration'),
  getConfig: () => api.get<KomfConfig>('/api/v1/komf/config'),
  patchConfig: (body: KomfConfigPatch) => api.patch<void>('/api/v1/komf/config', body),
  search: (params: { name: string; libraryId?: string; seriesId?: string }) =>
    api.get<KomfSeriesSearchResult[]>('/api/v1/komf/search', params),
  identify: (body: KomfIdentifyRequest) => api.post<KomfMetadataJobResponse>('/api/v1/komf/identify', body),
  matchSeries: (libraryId: string, seriesId: string) =>
    api.post<KomfMetadataJobResponse>(`/api/v1/komf/match/library/${libraryId}/series/${seriesId}`),
  // library-level match is a fire-and-forget background task on komf's side: no job id comes back
  matchLibrary: (libraryId: string) => api.post<void>(`/api/v1/komf/match/library/${libraryId}`),
  resetSeries: (libraryId: string, seriesId: string) =>
    api.post<void>(`/api/v1/komf/reset/library/${libraryId}/series/${seriesId}`),
  resetLibrary: (libraryId: string) => api.post<void>(`/api/v1/komf/reset/library/${libraryId}`),
  getJob: (jobId: string) => api.get<KomfJob>(`/api/v1/komf/jobs/${jobId}`),
  oauthStatus: (provider: KomfOAuthProvider) => api.get<KomfOAuthStatus>(`/api/v1/komf/oauth/${provider}/status`),
  oauthLogout: (provider: KomfOAuthProvider) => api.post<void>(`/api/v1/komf/oauth/${provider}/logout`),
  // start 302s to the provider's authorize page: a browser-navigation endpoint, never fetch it
  oauthStartUrl: (provider: KomfOAuthProvider) => `/api/v1/komf/oauth/${provider}/start`,
}
