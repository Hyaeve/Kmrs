import { api, ApiError } from './client'
import i18n from '@/lib/i18n'
import type {
  ActuatorInfo,
  ApiKeyDto,
  AuthenticationActivityDto,
  ClaimStatus,
  OAuth2ClientDto,
  Page,
  UserCreationDto,
  UserDto,
  UserUpdateDto,
} from './types'

function basicHeader(email: string, password: string): string {
  return `Basic ${btoa(unescape(encodeURIComponent(`${email}:${password}`)))}`
}

export const usersApi = {
  /** GET /users/me with the session cookie; resolves 401 when anonymous. */
  me: () => api.get<UserDto>('/api/v2/users/me'),

  /** Basic-auth login; rememberMe yields a persistent remember-me cookie. */
  login: async (email: string, password: string, rememberMe: boolean): Promise<UserDto> => {
    const res = await fetch(`/api/v2/users/me?remember-me=${rememberMe}`, {
      credentials: 'same-origin',
      headers: {
        'X-Requested-With': 'XMLHttpRequest',
        Authorization: basicHeader(email, password),
      },
    })
    if (res.status === 401) throw new ApiError(401, i18n.t('auth:invalidCredentials'))
    if (!res.ok) throw new ApiError(res.status, `${res.status} ${res.statusText}`)
    return (await res.json()) as UserDto
  },

  logout: () => api.post<void>('/api/logout'),

  updatePassword: (password: string) => api.patch<void>('/api/v2/users/me/password', { password }),

  apiKeys: () => api.get<ApiKeyDto[]>('/api/v2/users/me/api-keys'),
  createApiKey: (comment: string) => api.post<ApiKeyDto>('/api/v2/users/me/api-keys', { comment }),
  deleteApiKey: (keyId: string) => api.delete<void>(`/api/v2/users/me/api-keys/${keyId}`),

  oauth2Providers: () => api.get<OAuth2ClientDto[]>('/api/v1/oauth2/providers'),
  claimStatus: () => api.get<ClaimStatus>('/api/v1/claim'),

  authenticationActivity: (params?: { page?: number; size?: number }) =>
    api.get<Page<AuthenticationActivityDto>>('/api/v2/users/me/authentication-activity', params),

  // Admin operations
  list: () => api.get<UserDto[]>('/api/v2/users'),
  create: (body: UserCreationDto) => api.post<UserDto>('/api/v2/users', body),
  update: (userId: string, body: UserUpdateDto) => api.patch<void>(`/api/v2/users/${userId}`, body),
  delete: (userId: string) => api.delete<void>(`/api/v2/users/${userId}`),
  updatePasswordFor: (userId: string, password: string) =>
    api.patch<void>(`/api/v2/users/${userId}/password`, { password }),
  allAuthenticationActivity: (params?: { page?: number; size?: number }) =>
    api.get<Page<AuthenticationActivityDto>>('/api/v2/users/authentication-activity', params),
}

export const serverApi = {
  info: () => api.get<ActuatorInfo>('/actuator/info'),
}
