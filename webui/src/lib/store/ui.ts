import { create } from 'zustand'
import { persist } from 'zustand/middleware'

export type Theme = 'system' | 'light' | 'dark'
export type CardStyle = 'standard' | 'overlay' | 'cover'
export type GridDensity = 'compact' | 'standard' | 'cozy'

interface UiState {
  theme: Theme
  cardStyle: CardStyle
  gridDensity: GridDensity
  blurUnreadCovers: boolean
  sidebarLibrariesCollapsed: boolean
  /** search scope outside /search: 'all' | 'pinned' | a library id */
  searchScope: string
  setTheme: (t: Theme) => void
  setCardStyle: (s: CardStyle) => void
  setGridDensity: (d: GridDensity) => void
  setBlurUnreadCovers: (v: boolean) => void
  setSidebarLibrariesCollapsed: (v: boolean) => void
  setSearchScope: (v: string) => void
}

export const useUiStore = create<UiState>()(
  persist(
    (set) => ({
      theme: 'system',
      cardStyle: 'standard',
      gridDensity: 'standard',
      blurUnreadCovers: false,
      sidebarLibrariesCollapsed: false,
      searchScope: 'pinned',
      setTheme: (theme) => set({ theme }),
      setCardStyle: (cardStyle) => set({ cardStyle }),
      setGridDensity: (gridDensity) => set({ gridDensity }),
      setBlurUnreadCovers: (blurUnreadCovers) => set({ blurUnreadCovers }),
      setSidebarLibrariesCollapsed: (sidebarLibrariesCollapsed) => set({ sidebarLibrariesCollapsed }),
      setSearchScope: (searchScope) => set({ searchScope }),
    }),
    { name: 'kmweb.ui' },
  ),
)

export function resolveTheme(theme: Theme): 'light' | 'dark' {
  if (theme !== 'system') return theme
  return window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark'
}

export function applyTheme(theme: Theme) {
  const resolved = resolveTheme(theme)
  document.documentElement.classList.toggle('light', resolved === 'light')
  document.documentElement.classList.toggle('dark', resolved === 'dark')
  document
    .querySelector('meta[name="theme-color"]')
    ?.setAttribute('content', resolved === 'light' ? '#f7f7f5' : '#0b0b0d')
}

/** Density multiplies the base card width (kmreader: 0.8 / 1.0 / 1.3). */
export function densityScale(d: GridDensity): number {
  return d === 'compact' ? 0.8 : d === 'cozy' ? 1.3 : 1
}

export function useDensityCardWidth(base = 140): number {
  const density = useUiStore((s) => s.gridDensity)
  return Math.round(base * densityScale(density))
}
