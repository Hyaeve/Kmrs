import { Navigate, Outlet } from 'react-router-dom'
import { isAdmin, useAuthStore } from '@/lib/store/auth'

/** Nested under RequireAuth, so auth status is already resolved here. */
export function RequireAdmin() {
  const user = useAuthStore((s) => s.user)
  if (!isAdmin(user)) return <Navigate to="/dashboard" replace />
  return <Outlet />
}
