import { useCallback } from 'react'
import { useSearchParams } from 'react-router-dom'
import i18n from '@/lib/i18n'
import { useLibraryPrefs } from '@/lib/store/libraryPrefs'
import type { SortOption, SortState } from './types'

export function parseSort(raw: string | null | undefined): SortState | undefined {
  if (!raw) return undefined
  const i = raw.lastIndexOf(',')
  if (i <= 0) return undefined
  const dir = raw.slice(i + 1)
  if (dir !== 'asc' && dir !== 'desc') return undefined
  return { property: raw.slice(0, i), direction: dir }
}

export function serializeSort(s: SortState): string {
  return `${s.property},${s.direction}`
}

export function sortLabel(options: SortOption[], property: string): string {
  const found = options.find((o) => o.property === property)
  if (found) return i18n.t(found.labelKey)
  return property === 'name' ? i18n.t('filters:sort.fileName') : property
}

/** name/number-like fields read naturally ascending; everything else (dates, counts) newest/largest first */
export function defaultDirection(property: string): 'asc' | 'desc' {
  return property === 'metadata.titleSort' ||
    property === 'name' ||
    property === 'series' ||
    property === 'url' ||
    property === 'metadata.numberSort' ||
    property === 'readList.number' ||
    property === 'collection.number'
    ? 'asc'
    : 'desc'
}

export function useSortState(scope: string, fallback: SortState) {
  const [searchParams, setSearchParams] = useSearchParams()
  const remembered = useLibraryPrefs((s) => s.sort[scope])
  const setSortPref = useLibraryPrefs((s) => s.setSort)

  const current = parseSort(searchParams.get('sort')) ?? parseSort(remembered) ?? fallback

  const set = useCallback(
    (next: SortState) => {
      setSortPref(scope, serializeSort(next))
      const params = new URLSearchParams(searchParams)
      params.set('sort', serializeSort(next))
      setSearchParams(params)
    },
    [scope, searchParams, setSearchParams, setSortPref],
  )

  return { current, set }
}
