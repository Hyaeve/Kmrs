import { create } from 'zustand'
import { persist } from 'zustand/middleware'

interface LibraryPrefs {
  /** last sort per browse scope (e.g. "series:<libraryId>", "books:<libraryId>") */
  sort: Record<string, string>
}

interface LibraryPrefsState extends LibraryPrefs {
  setSort: (scope: string, sort: string) => void
}

export const useLibraryPrefs = create<LibraryPrefsState>()(
  persist(
    (set) => ({
      sort: {},
      setSort: (scope, sort) => set((s) => ({ sort: { ...s.sort, [scope]: sort } })),
    }),
    { name: 'kmweb.libraryPrefs' },
  ),
)
