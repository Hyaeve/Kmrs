import { api } from './client'
import type { ServerInfoDto, ServerScheduledTaskDto, ServerSessionDto } from './types'

export const serverApi = {
  info: () => api.get<ServerInfoDto>('/api/v1/server/info'),
  scheduledTasks: () => api.get<{ tasks: ServerScheduledTaskDto[] }>('/api/v1/server/scheduled-tasks'),
  sessions: () => api.get<{ sessions: ServerSessionDto[] }>('/api/v1/server/sessions'),
  deleteSession: (sessionId: string) => api.delete<void>(`/api/v1/server/sessions/${sessionId}`),
  logfileUrl: () => '/api/v1/server/logfile',
}
