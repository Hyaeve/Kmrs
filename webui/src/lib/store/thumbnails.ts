import { create } from 'zustand'

// Bust counters for cover URLs, bumped on SSE Thumbnail* events so <img>
// reloads without a full page refresh.
interface ThumbnailState {
  bust: Record<string, number>
  bump: (id: string) => void
}

export const useThumbnailBust = create<ThumbnailState>()((set) => ({
  bust: {},
  bump: (id) => set((s) => ({ bust: { ...s.bust, [id]: (s.bust[id] ?? 0) + 1 } })),
}))

export const useBust = (id: string) => useThumbnailBust((s) => s.bust[id] ?? 0)
