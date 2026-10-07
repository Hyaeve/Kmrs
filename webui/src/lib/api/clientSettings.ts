import { api } from './client'
import type { ClientSettingDto, ClientSettingGlobalUpdateDto, ClientSettingUserUpdateDto } from './types'

export const clientSettingsApi = {
  listGlobal: () => api.get<Record<string, ClientSettingDto>>('/api/v1/client-settings/global/list'),
  listUser: () => api.get<Record<string, ClientSettingDto>>('/api/v1/client-settings/user/list'),

  /** Admin only. Keys must match the server's lowercase dotted-key pattern. */
  saveGlobal: (settings: Record<string, ClientSettingGlobalUpdateDto>) =>
    api.patch<void>('/api/v1/client-settings/global', settings),
  saveUser: (settings: Record<string, ClientSettingUserUpdateDto>) =>
    api.patch<void>('/api/v1/client-settings/user', settings),

  deleteGlobal: (keys: string[]) => api.delete<void>('/api/v1/client-settings/global', keys),
  deleteUser: (keys: string[]) => api.delete<void>('/api/v1/client-settings/user', keys),
}
