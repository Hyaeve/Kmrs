import { create } from 'zustand'

export interface Toast {
  id: number
  message: string
}

interface ToastState {
  toast: Toast | null
  show: (message: string, duration?: number) => void
}

// timer and id live outside the store so a new toast can replace the old one
// without round-tripping through state
let timer: number | undefined
let nextId = 0

export const useToastStore = create<ToastState>()((set) => ({
  toast: null,
  show: (message, duration = 3000) => {
    window.clearTimeout(timer)
    set({ toast: { id: ++nextId, message } })
    timer = window.setTimeout(() => set({ toast: null }), duration)
  },
}))

export function showToast(message: string, duration?: number) {
  useToastStore.getState().show(message, duration)
}
