import type { PageParams } from './types'

export class ApiError extends Error {
  constructor(
    public status: number,
    message: string,
    public body?: unknown,
  ) {
    super(message)
    this.name = 'ApiError'
  }
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, {
    credentials: 'same-origin',
    ...init,
    headers: {
      'X-Requested-With': 'XMLHttpRequest',
      // FormData bodies must keep the browser-set multipart content type with its boundary
      ...(init?.body && !(init.body instanceof FormData) ? { 'Content-Type': 'application/json' } : {}),
      ...init?.headers,
    },
  })
  if (res.status === 204 || res.status === 205) return undefined as T
  if (!res.ok) {
    let body: unknown
    const text = await res.text().catch(() => '')
    if (text) {
      try {
        body = JSON.parse(text)
      } catch {
        body = text
      }
    }
    const msg =
      body && typeof body === 'object' && 'message' in body && typeof (body as { message: unknown }).message === 'string'
        ? ((body as { message: string }).message as string)
        : `${res.status} ${res.statusText}`
    throw new ApiError(res.status, msg, body)
  }
  const text = await res.text()
  if (!text) return undefined as T
  return JSON.parse(text) as T
}

/** Serialize page/sort params; multi-value params repeat the key (Spring style). */
export function withParams(path: string, params?: Record<string, unknown>): string {
  if (!params) return path
  const qs = new URLSearchParams()
  for (const [key, value] of Object.entries(params)) {
    if (value === undefined || value === null || value === '') continue
    if (Array.isArray(value)) {
      for (const v of value) qs.append(key, String(v))
    } else {
      qs.append(key, String(value))
    }
  }
  const s = qs.toString()
  return s ? `${path}?${s}` : path
}

export function pageQuery(params?: PageParams): Record<string, unknown> {
  if (!params) return {}
  const out: Record<string, unknown> = {}
  if (params.page !== undefined) out.page = params.page
  if (params.size !== undefined) out.size = params.size
  if (params.unpaged) out.unpaged = true
  if (params.sort?.length) out.sort = params.sort
  return out
}

export const api = {
  get: <T>(path: string, params?: Record<string, unknown>) => request<T>(withParams(path, params)),
  post: <T>(path: string, body?: unknown, params?: Record<string, unknown>) =>
    request<T>(withParams(path, params), { method: 'POST', body: body === undefined ? undefined : JSON.stringify(body) }),
  patch: <T>(path: string, body?: unknown) => request<T>(path, { method: 'PATCH', body: JSON.stringify(body) }),
  put: <T>(path: string, body?: unknown, params?: Record<string, unknown>) =>
    request<T>(withParams(path, params), { method: 'PUT', body: body === undefined ? undefined : JSON.stringify(body) }),
  delete: <T>(path: string, body?: unknown, params?: Record<string, unknown>) =>
    request<T>(withParams(path, params), { method: 'DELETE', body: body === undefined ? undefined : JSON.stringify(body) }),
  upload: <T>(path: string, form: FormData) => request<T>(path, { method: 'POST', body: form }),
}
