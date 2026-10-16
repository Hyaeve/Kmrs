import type { EpubFontFamily } from '@/lib/store/readerSettings'

export const serverFontValue = (family: string): EpubFontFamily => `server:${family}`

export function isServerFont(value: EpubFontFamily): value is `server:${string}` {
  return value.startsWith('server:')
}

/** CSS font-family stack for a setting value; null keeps the publisher's fonts. */
export function epubFontStack(value: EpubFontFamily): string | null {
  if (value === 'Original') return null
  if (isServerFont(value)) return `'${value.slice('server:'.length)}', Georgia, 'Songti SC', 'Noto Serif CJK SC', serif`
  return null
}
