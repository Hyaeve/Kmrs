import { create } from 'zustand'

export type UpdateServiceWorker = (reloadPage?: boolean) => Promise<void>

interface PwaState {
  updateServiceWorker: UpdateServiceWorker | null
  showUpdate: (update: UpdateServiceWorker) => void
  dismissUpdate: () => void
}

export const usePwaStore = create<PwaState>()((set) => ({
  updateServiceWorker: null,
  showUpdate: (updateServiceWorker) => set({ updateServiceWorker }),
  dismissUpdate: () => set({ updateServiceWorker: null }),
}))
