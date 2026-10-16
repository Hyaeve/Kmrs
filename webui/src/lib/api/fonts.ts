import { api } from './client'

export const fontsApi = {
  families: () => api.get<string[]>('/api/v1/fonts/families'),
  cssUrl: (family: string) => `/api/v1/fonts/resource/${encodeURIComponent(family)}/css`,
}
