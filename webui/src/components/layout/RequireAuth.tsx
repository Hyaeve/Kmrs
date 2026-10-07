import { Navigate, Outlet, ScrollRestoration, useLocation } from 'react-router-dom'
import { useAuthStore } from '@/lib/store/auth'
import { useAuthBootstrap } from '@/lib/hooks/useAuthBootstrap'
import { useSseWiring } from '@/lib/hooks/useSseWiring'
import { LogoMark } from '@/components/LogoMark'

function Splash() {
  return (
    <div className="flex min-h-dvh flex-col items-center justify-center gap-4 bg-bg">
      <LogoMark className="size-14 animate-pulse" />
    </div>
  )
}

export function RequireAuth() {
  useAuthBootstrap()
  useSseWiring()
  const status = useAuthStore((s) => s.status)
  const location = useLocation()

  if (status === 'unknown') return <Splash />
  if (status === 'anonymous') {
    const redirect = encodeURIComponent(location.pathname + location.search)
    return <Navigate to={`/login?redirect=${redirect}`} replace />
  }
  return (
    <>
      {/* the browser's own restoration scrolls the still-rendered old page on POP, flashing it before the new route paints */}
      <ScrollRestoration />
      <Outlet />
    </>
  )
}
