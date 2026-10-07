import { ApiError } from '@/lib/api/client'
import type { PairErrors } from './fields'

export function isValidUrl(value: string): boolean {
  return /^[A-Za-z0-9+.-]+:.+$/.test(value)
}

export function isValidBcp47(tag: string): boolean {
  try {
    return Intl.getCanonicalLocales(tag).length > 0
  } catch {
    return false
  }
}

/** ISBN-13 per the server validator: 978/979 prefix plus check digit, hyphens and spaces ignored. */
export function isValidIsbn13(value: string): boolean {
  const digits = [...value].filter((c) => c >= '0' && c <= '9')
  if (digits.length !== 13) return false
  const stripped = digits.join('')
  if (!stripped.startsWith('978') && !stripped.startsWith('979')) return false
  const nums = digits.map(Number)
  const sum = nums.slice(0, 12).reduce((acc, d, i) => acc + (i % 2 === 0 ? d : d * 3), 0)
  return (10 - (sum % 10)) % 10 === nums[12]
}

export interface ViolationMessage {
  field: string
  message: string
}

export function violationMessages(err: unknown): ViolationMessage[] {
  if (!(err instanceof ApiError)) return []
  const body = err.body as { violations?: { fieldName?: unknown; message?: unknown }[] } | undefined
  if (!body || !Array.isArray(body.violations)) return []
  return body.violations
    .filter((v) => typeof v?.message === 'string')
    .map((v) => ({ field: typeof v.fieldName === 'string' ? v.fieldName : '', message: v.message as string }))
}

const PAIR_SUBFIELD: Record<string, 'a' | 'b'> = { label: 'a', name: 'a', url: 'b', title: 'b', role: 'b' }

export interface MappedViolations {
  scalars: Record<string, string>
  pairs: Record<string, PairErrors>
  rest: string[]
}

/** Splits server violations into scalar field errors, pair row errors (`links[0].url`), and leftovers. */
export function mapViolations(err: unknown): MappedViolations {
  const scalars: Record<string, string> = {}
  const pairs: Record<string, PairErrors> = {}
  const rest: string[] = []
  for (const v of violationMessages(err)) {
    const m = v.field.match(/^(\w+)\[(\d+)\]\.(\w+)$/)
    if (m && PAIR_SUBFIELD[m[3]]) {
      const [, list, index, sub] = m
      pairs[list] ??= {}
      pairs[list][Number(index)] = { ...pairs[list][Number(index)], [PAIR_SUBFIELD[sub]]: v.message }
    } else if (v.field && !v.field.includes('[')) {
      scalars[v.field] = v.message
    } else {
      rest.push(v.field ? `${v.field}: ${v.message}` : v.message)
    }
  }
  return { scalars, pairs, rest }
}
