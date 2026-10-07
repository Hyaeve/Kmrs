import { useEffect } from 'react'
import { usersApi } from '@/lib/api/users'
import { ApiError } from '@/lib/api/client'
import { useAuthStore } from '@/lib/store/auth'

/** One-shot session bootstrap: resolves the cookie session into auth state. */
export function useAuthBootstrap() {
  const status = useAuthStore((s) => s.status)
  const setUser = useAuthStore((s) => s.setUser)
  const setAnonymous = useAuthStore((s) => s.setAnonymous)

  useEffect(() => {
    if (status !== 'unknown') return
    let cancelled = false
    usersApi
      .me()
      .then((user) => {
        if (!cancelled) setUser(user)
      })
      .catch((e) => {
        if (cancelled) return
        if (e instanceof ApiError && e.status === 401) setAnonymous()
        else setAnonymous()
      })
    return () => {
      cancelled = true
    }
  }, [status, setUser, setAnonymous])
}
