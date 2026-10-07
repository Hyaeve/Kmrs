import { useMemo } from 'react'
import {
  dashboardSectionsKey,
  parseDashboardSectionConfig,
  useSetUserSetting,
  useUserClientSettings,
  type DashboardSectionConfigEntry,
} from '@/lib/store/clientSettings'
import { DASHBOARD_SECTIONS, type DashboardSectionKey } from './sections'

export const DEFAULT_SECTION_ORDER = Object.keys(DASHBOARD_SECTIONS) as DashboardSectionKey[]

export interface ResolvedDashboardSection {
  key: DashboardSectionKey
  hidden: boolean
}

/** stored order wins; unknown keys are dropped, missing keys keep the default order at the end */
export function resolveDashboardSections(config: DashboardSectionConfigEntry[] | undefined): ResolvedDashboardSection[] {
  const seen = new Map<DashboardSectionKey, boolean>()
  const ordered: DashboardSectionKey[] = []
  for (const entry of config ?? []) {
    if (entry.key in DASHBOARD_SECTIONS && !seen.has(entry.key as DashboardSectionKey)) {
      const key = entry.key as DashboardSectionKey
      ordered.push(key)
      seen.set(key, !!entry.hidden)
    }
  }
  for (const key of DEFAULT_SECTION_ORDER) {
    if (!seen.has(key)) {
      ordered.push(key)
      seen.set(key, false)
    }
  }
  return ordered.map((key) => ({ key, hidden: seen.get(key) ?? false }))
}

/** library filter for section fetches: undefined aggregates every library; pinned [] is handled by callers */
export function dashboardLibraryIds(libraryId: string | undefined, pinned: string[] | undefined): string[] | undefined {
  if (libraryId) return [libraryId]
  return pinned
}

/** query-key scope shared by the dashboard rows and the section full page so they hit the same cache */
export function dashboardScope(libraryId: string | undefined, pinned: string[] | undefined): string {
  if (libraryId) return libraryId
  if (!pinned) return 'all'
  return pinned.length ? `pinned:${pinned.join(',')}` : 'pinned:none'
}

export function useDashboardSections(libraryId?: string) {
  const settingsQuery = useUserClientSettings()
  const key = dashboardSectionsKey(libraryId)
  const raw = settingsQuery.data?.[key]?.value
  const config = useMemo(() => parseDashboardSectionConfig(raw), [raw])
  const sections = useMemo(() => resolveDashboardSections(config), [config])

  const mutation = useSetUserSetting()
  const save = (next: ResolvedDashboardSection[]) =>
    mutation.mutate({ key, value: JSON.stringify(next.map((s) => ({ key: s.key, hidden: s.hidden }))) })
  const reset = () => mutation.mutate({ key, value: null })

  return { sections, save, reset, isLoading: settingsQuery.isPending, isSaving: mutation.isPending }
}
