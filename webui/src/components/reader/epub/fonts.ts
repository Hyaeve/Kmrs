import type { EpubFontFamily } from '@/lib/store/readerSettings'

export const serverFontValue = (family: string): EpubFontFamily => `server:${family}`
export const localFontValue = (family: string): EpubFontFamily => `local:${family}`

export function isServerFont(value: EpubFontFamily): value is `server:${string}` {
  return value.startsWith('server:')
}

export function isLocalFont(value: EpubFontFamily): value is `local:${string}` {
  return value.startsWith('local:')
}

export function localFontName(value: `local:${string}`): string {
  return value.slice('local:'.length)
}

function quoteFontFamily(family: string): string {
  return `'${family.replace(/\\/g, '\\\\').replace(/'/g, "\\'")}'`
}

/** CSS font-family stack for a setting value; null keeps the publisher's fonts. */
export function epubFontStack(value: EpubFontFamily): string | null {
  if (value === 'Original') return null
  if (isServerFont(value) || isLocalFont(value)) {
    const family = value.slice(value.indexOf(':') + 1)
    return `${quoteFontFamily(family)}, Georgia, 'Songti SC', 'Noto Serif CJK SC', serif`
  }
  return null
}

export async function querySystemFontFamilies(): Promise<string[] | null> {
  const target = window as Window & {
    queryLocalFonts?: () => Promise<Array<{ family: string }>>
  }
  if (!target.queryLocalFonts) return null
  const fonts = await target.queryLocalFonts()
  return [...new Set(fonts.map((font) => font.family))].sort((a, b) => a.localeCompare(b))
}
