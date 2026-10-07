import type { AuthorDto } from '@/lib/api/types'

// writers first, custom roles last (KMReader order)
const ROLE_ORDER = ['writer', 'penciller', 'inker', 'colorist', 'letterer', 'cover', 'editor', 'translator']

export function sortAuthorsByRole(authors: AuthorDto[]): AuthorDto[] {
  return [...authors].sort((a, b) => roleRank(a.role) - roleRank(b.role))
}

/** the one author worth surfacing where only a single name fits (card text lines) */
export function primaryAuthor(authors: AuthorDto[]): AuthorDto | undefined {
  return sortAuthorsByRole(authors)[0]
}

function roleRank(role: string): number {
  const i = ROLE_ORDER.indexOf(role.toLowerCase())
  return i < 0 ? ROLE_ORDER.length : i
}
