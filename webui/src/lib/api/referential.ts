import { api } from './client'
import type { AuthorDto, Page } from './types'

// v2 referential endpoints return paged payloads; filter UIs want the full list (unpaged).
export interface ReferentialScope {
  libraryId?: string[]
  collectionId?: string
  seriesId?: string
  readListId?: string
}

/** the library/collection-only endpoints ignore series and read list ids */
export type LibraryCollectionScope = Pick<ReferentialScope, 'libraryId' | 'collectionId'>

// the server applies library > collection precedence and ignores the less specific param,
// so only the most specific applicable scope is sent
function lcParams(scope?: LibraryCollectionScope): Record<string, unknown> {
  if (scope?.libraryId?.length) return { library_id: scope.libraryId }
  return { collection_id: scope?.collectionId }
}

// tags and authors accept the full scope (library > collection > series > read list on the server)
function fullParams(scope?: ReferentialScope): Record<string, unknown> {
  if (scope?.seriesId) return { series_id: scope.seriesId }
  if (scope?.readListId) return { readlist_id: scope.readListId }
  return lcParams(scope)
}

const unpaged = { unpaged: true }

export const referentialApi = {
  genres: (scope?: LibraryCollectionScope) =>
    api.get<Page<string>>('/api/v2/genres', { ...lcParams(scope), ...unpaged }).then((p) => p.content),
  // series-level tag options: BOTH reads the combined series + aggregated book tag view, so
  // oneshots (whose tags live on the book) contribute too
  tags: (scope?: ReferentialScope) =>
    api
      .get<Page<string>>('/api/v2/tags', { ...fullParams(scope), include: 'BOTH', ...unpaged })
      .then((p) => p.content),
  bookTags: (scope?: ReferentialScope) =>
    api
      .get<Page<string>>('/api/v2/tags', { ...fullParams(scope), include: 'BOOK', ...unpaged })
      .then((p) => p.content),
  publishers: (scope?: LibraryCollectionScope) =>
    api.get<Page<string>>('/api/v2/publishers', { ...lcParams(scope), ...unpaged }).then((p) => p.content),
  ageRatings: (scope?: LibraryCollectionScope) =>
    api
      .get<Page<number>>('/api/v2/age-ratings', { ...lcParams(scope), ...unpaged })
      .then((p) => p.content.map(String)),
  languages: (scope?: LibraryCollectionScope) =>
    api.get<Page<string>>('/api/v2/languages', { ...lcParams(scope), ...unpaged }).then((p) => p.content),
  sharingLabels: (scope?: LibraryCollectionScope) =>
    api.get<Page<string>>('/api/v2/sharing-labels', { ...lcParams(scope), ...unpaged }).then((p) => p.content),
  releaseDates: (scope?: LibraryCollectionScope) =>
    api.get<Page<string>>('/api/v2/series/release-years', { ...lcParams(scope), ...unpaged }).then((p) => p.content),
  authors: (scope?: ReferentialScope) =>
    api.get<Page<AuthorDto>>('/api/v2/authors', { ...fullParams(scope), ...unpaged }).then((p) => p.content),
}
