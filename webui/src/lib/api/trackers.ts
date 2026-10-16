import { api } from './client'
import type {
  KomfOAuthProvider,
  KomfOAuthStatus,
  TrackerLink,
  TrackerLinkUpsert,
  TrackerPreferences,
  TrackerSearchItem,
  TrackerState,
  TrackerUpdatePayload,
} from './types'

/** TanStack Query keys for the tracker bindings; shared by every consumer. */
export const trackerLinkKeys = {
  all: ['trackers', 'links'] as const,
  series: (seriesId: string) => ['trackers', 'links', seriesId] as const,
  preferences: ['trackers', 'preferences'] as const,
}

/** Platform entry page URL; mirrors komf's `tracker_entry_url` mapping. */
export function trackerEntryUrl(provider: KomfOAuthProvider, trackId: string): string {
  switch (provider) {
    case 'anilist':
      return `https://anilist.co/manga/${trackId}`
    case 'mal':
      return `https://myanimelist.net/manga/${trackId}`
    case 'bangumi':
      return `https://bgm.tv/subject/${trackId}`
    case 'mangabaka':
      return `https://mangabaka.org/${trackId}`
  }
}

const METADATA_LINK_PATTERNS: Array<[KomfOAuthProvider, RegExp]> = [
  ['anilist', /anilist\.co\/manga\/(\d+)/],
  ['mal', /myanimelist\.net\/manga\/(\d+)/],
  ['bangumi', /(?:bgm\.tv|bangumi\.tv)\/subject\/(\d+)/],
  ['mangabaka', /mangabaka\.org\/(?:series\/)?(\d+)/],
]

/** Finds platform entry links in series metadata links (komf-matched hosts);
 * the returned URL is fed to komf's search box, which resolves links directly. */
export function trackerUrlsFromMetadata(links: Array<{ url: string }>): Partial<Record<KomfOAuthProvider, string>> {
  const out: Partial<Record<KomfOAuthProvider, string>> = {}
  for (const { url } of links) {
    for (const [provider, pattern] of METADATA_LINK_PATTERNS) {
      if (out[provider]) continue
      if (pattern.test(url)) out[provider] = url
    }
  }
  return out
}

export const trackersApi = {
  // bindings are kmrs-side state, scoped to the calling user
  listLinks: () => api.get<TrackerLink[]>('/api/v1/komf/trackers/links'),
  listSeriesLinks: (seriesId: string) => api.get<TrackerLink[]>(`/api/v1/komf/trackers/links/${seriesId}`),
  upsertLink: (seriesId: string, body: TrackerLinkUpsert) =>
    api.put<TrackerLink>(`/api/v1/komf/trackers/links/${seriesId}`, body),
  deleteLink: (seriesId: string, provider: KomfOAuthProvider) =>
    api.delete<void>(`/api/v1/komf/trackers/links/${seriesId}/${provider}`),

  // per-user display preferences (kmrs-side state)
  getPreferences: () => api.get<TrackerPreferences>('/api/v1/komf/trackers/preferences'),
  putPreferences: (body: TrackerPreferences) =>
    api.put<TrackerPreferences>('/api/v1/komf/trackers/preferences', body),

  // komf passthroughs, scoped by the server to the calling user's tracker account
  search: (provider: KomfOAuthProvider, params: { name: string; nsfw?: boolean }) =>
    api.get<TrackerSearchItem[]>(`/api/v1/komf/trackers/${provider}/search`, params),
  getState: (provider: KomfOAuthProvider, trackId: string) =>
    api.get<TrackerState>(`/api/v1/komf/trackers/${provider}/state`, { trackId }),
  update: (provider: KomfOAuthProvider, body: TrackerUpdatePayload) =>
    api.post<void>(`/api/v1/komf/trackers/${provider}/update`, body),

  oauthStatus: (provider: KomfOAuthProvider) =>
    api.get<KomfOAuthStatus>(`/api/v1/komf/trackers/oauth/${provider}/status`),
  oauthLogout: (provider: KomfOAuthProvider) =>
    api.post<void>(`/api/v1/komf/trackers/oauth/${provider}/logout`),
  // start 302s to the provider's authorize page: a browser-navigation endpoint, never fetch it
  oauthStartUrl: (provider: KomfOAuthProvider) => `/api/v1/komf/trackers/oauth/${provider}/start`,
}
