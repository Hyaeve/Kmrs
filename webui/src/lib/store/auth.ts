import { create } from 'zustand'
import type { UserDto } from '@/lib/api/types'

export type AuthStatus = 'unknown' | 'authenticated' | 'anonymous'

interface AuthState {
  status: AuthStatus
  user: UserDto | null
  setUser: (user: UserDto) => void
  setAnonymous: () => void
  clear: () => void
}

export const useAuthStore = create<AuthState>()((set) => ({
  status: 'unknown',
  user: null,
  setUser: (user) => set({ status: 'authenticated', user }),
  setAnonymous: () => set({ status: 'anonymous', user: null }),
  clear: () => set({ status: 'anonymous', user: null }),
}))

export const isAdmin = (user: UserDto | null): boolean => !!user?.roles.includes('ADMIN')
export const isOwner = (user: UserDto | null, ownerId: string): boolean => !!user && user.id === ownerId
export const canDownload = (user: UserDto | null): boolean =>
  !!user && (user.roles.includes('ADMIN') || user.roles.includes('FILE_DOWNLOAD'))
export const canStream = (user: UserDto | null): boolean =>
  !!user && (user.roles.includes('ADMIN') || user.roles.includes('PAGE_STREAMING'))
