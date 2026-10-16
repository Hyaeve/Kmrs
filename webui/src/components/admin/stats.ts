import { useQuery } from '@tanstack/react-query'
import { statsApi } from '@/lib/api/stats'

/** Shared keys: the admin panels poll one deduplicated request per endpoint. */
export function useServerStats() {
  return useQuery({ queryKey: ['admin', 'stats', 'server'], queryFn: statsApi.server, refetchInterval: 30_000 })
}

export function useLibrariesStats() {
  return useQuery({ queryKey: ['admin', 'stats', 'libraries'], queryFn: statsApi.libraries, refetchInterval: 30_000 })
}
