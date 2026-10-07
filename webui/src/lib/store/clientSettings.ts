import { useMemo } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { clientSettingsApi } from '@/lib/api/clientSettings'
import type { ClientSettingDto } from '@/lib/api/types'

const USER_SETTINGS_KEY = ['client-settings', 'user'] as const

export const PINNED_LIBRARIES_KEY = 'webui.pinned.libraries'
export const DASHBOARD_SECTIONS_KEY = 'webui.dashboard.sections'

// library ids are uppercase TSIDs but the server only accepts lowercase dotted keys
export const dashboardSectionsKey = (libraryId?: string) =>
  libraryId ? `${DASHBOARD_SECTIONS_KEY}.${libraryId.toLowerCase()}` : DASHBOARD_SECTIONS_KEY

export function useUserClientSettings() {
  return useQuery({ queryKey: USER_SETTINGS_KEY, queryFn: clientSettingsApi.listUser })
}

/** value null deletes the key, returning to the never-configured state */
export function useSetUserSetting() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ key, value }: { key: string; value: string | null }) =>
      value === null ? clientSettingsApi.deleteUser([key]) : clientSettingsApi.saveUser({ [key]: { value } }),
    onMutate: async ({ key, value }) => {
      await queryClient.cancelQueries({ queryKey: USER_SETTINGS_KEY })
      const previous = queryClient.getQueryData<Record<string, ClientSettingDto>>(USER_SETTINGS_KEY)
      queryClient.setQueryData<Record<string, ClientSettingDto>>(USER_SETTINGS_KEY, (old) => {
        const next = { ...old }
        if (value === null) delete next[key]
        else next[key] = { ...next[key], value }
        return next
      })
      return { previous }
    },
    onError: (_error, _vars, context) => {
      queryClient.setQueryData(USER_SETTINGS_KEY, context?.previous)
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: USER_SETTINGS_KEY }),
  })
}

function parseJson(raw: string | undefined): unknown {
  if (raw === undefined) return undefined
  try {
    return JSON.parse(raw)
  } catch {
    return undefined
  }
}

/** undefined means the key was never set: every library is implicitly visible */
export function parsePinnedLibraries(raw: string | undefined): string[] | undefined {
  const parsed = parseJson(raw)
  if (!Array.isArray(parsed)) return undefined
  return parsed.filter((id): id is string => typeof id === 'string')
}

export function usePinnedLibraries() {
  const query = useUserClientSettings()
  const raw = query.data?.[PINNED_LIBRARIES_KEY]?.value
  const pinned = useMemo(() => parsePinnedLibraries(raw), [raw])
  return { ...query, pinned }
}

export function useSetPinnedLibraries() {
  const mutation = useSetUserSetting()
  return {
    ...mutation,
    setPinned: (libraryIds: string[]) =>
      mutation.mutate({ key: PINNED_LIBRARIES_KEY, value: JSON.stringify(libraryIds) }),
    resetPinned: () => mutation.mutate({ key: PINNED_LIBRARIES_KEY, value: null }),
  }
}

export interface DashboardSectionConfigEntry {
  key: string
  hidden?: boolean
}

export function parseDashboardSectionConfig(raw: string | undefined): DashboardSectionConfigEntry[] | undefined {
  const parsed = parseJson(raw)
  if (!Array.isArray(parsed)) return undefined
  return parsed.filter(
    (entry): entry is DashboardSectionConfigEntry =>
      !!entry && typeof entry === 'object' && typeof (entry as DashboardSectionConfigEntry).key === 'string',
  )
}
