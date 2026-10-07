import { useQuery } from '@tanstack/react-query'
import { komfApi } from '@/lib/api/komf'
import { isAdmin, useAuthStore } from '@/lib/store/auth'

/** komf actions are admin-only and only work once the integration is connected;
    every entry point shares the admin page's cached integration query. */
export function useKomfIntegration(): boolean {
  const user = useAuthStore((s) => s.user)
  const admin = isAdmin(user)
  const query = useQuery({
    queryKey: ['admin', 'komf-integration'],
    queryFn: komfApi.getIntegration,
    // the endpoint is admin-only; asking as a regular user is a guaranteed 403
    enabled: admin,
  })
  return query.data?.state === 'connected'
}
