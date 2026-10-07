import { useSearchParams } from 'react-router-dom'
import { usePinnedLibraries } from '@/lib/store/clientSettings'
import { useUiStore } from '@/lib/store/ui'

export type SearchScope = { kind: 'all' } | { kind: 'pinned'; ids: string[] } | { kind: 'library'; id: string }

// 'pinned' resolves only when pins exist; anything unrecognized falls back to a global search
export function parseScope(raw: string | null, pinned: string[] | undefined): SearchScope {
  if (raw === 'pinned' && pinned && pinned.length > 0) return { kind: 'pinned', ids: pinned }
  if (raw && raw !== 'all' && raw !== 'pinned') return { kind: 'library', id: raw }
  return { kind: 'all' }
}

export function scopeLibraryIds(scope: SearchScope): string[] | undefined {
  if (scope.kind === 'all') return undefined
  return scope.kind === 'pinned' ? scope.ids : [scope.id]
}

export function scopeKey(scope: SearchScope): string {
  if (scope.kind === 'all') return 'all'
  return scope.kind === 'pinned' ? `pinned:${scope.ids.join(',')}` : scope.id
}

export function scopeValue(scope: SearchScope): string {
  if (scope.kind === 'all') return 'all'
  return scope.kind === 'pinned' ? 'pinned' : scope.id
}

// scope for searches started outside /search, where the URL carries no ?scope=
// param; pure frontend state, defaults to the pinned libraries
export function useSearchScopeDefault() {
  const stored = useUiStore((s) => s.searchScope)
  const setStored = useUiStore((s) => s.setSearchScope)
  const { pinned } = usePinnedLibraries()
  const value = scopeValue(parseScope(stored, pinned))
  return { value, setValue: setStored }
}

// the ?scope= param is the single source of truth, shared by the header scope
// menu and the search page's queries
export function useSearchScope() {
  const [searchParams, setSearchParams] = useSearchParams()
  const { pinned } = usePinnedLibraries()
  const scope = parseScope(searchParams.get('scope'), pinned)

  const setScope = (value: string) => {
    setSearchParams(
      (prev) => {
        const next = new URLSearchParams(prev)
        if (value === 'all') next.delete('scope')
        else next.set('scope', value)
        return next
      },
      { replace: true },
    )
  }

  return { scope, setScope }
}
