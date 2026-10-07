import { create } from 'zustand'
import type { TaskQueueStatus } from '@/lib/api/types'

// Latest TaskQueueStatus from the admin SSE stream, shared so the header badge
// and the admin panel read a single subscription wired in useSseWiring.
interface TaskQueueState {
  status: TaskQueueStatus | null
  setStatus: (status: TaskQueueStatus) => void
}

export const useTaskQueue = create<TaskQueueState>()((set) => ({
  status: null,
  setStatus: (status) => set({ status }),
}))
