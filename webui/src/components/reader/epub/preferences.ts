import { EpubPreferences, type IEpubPreferences } from '@readium/navigator'
import { Locator, type Publication } from '@readium/shared'
import type { EpubFontFamily, EpubTheme } from '@/lib/store/readerSettings'
import { epubFontStack } from './fonts'

type ThemeColors = Pick<IEpubPreferences, 'backgroundColor' | 'textColor' | 'linkColor' | 'visitedColor'>

// DAY clears the color overrides so publisher styles win
const THEME_COLORS: Record<EpubTheme, ThemeColors> = {
  DAY: { backgroundColor: null, textColor: null, linkColor: null, visitedColor: null },
  SEPIA: { backgroundColor: '#e9ddc8', textColor: '#000000', linkColor: '#0000ee', visitedColor: '#551a8b' },
  NIGHT: { backgroundColor: '#000000', textColor: '#fefefe', linkColor: '#63caff', visitedColor: '#0099e5' },
}

// the navigator caps the container to its line-length layout, so the slack around it shows this backdrop
export const EPUB_BACKDROP: Record<EpubTheme, string> = {
  DAY: '#ffffff',
  SEPIA: '#e9ddc8',
  NIGHT: '#000000',
}

// Readium's 65-char default line length is tuned for Latin; at ~1em per glyph it puts the two-column threshold past tablet widths for CJK books
export const CJK_OPTIMAL_LINE_LENGTH = 40

export function buildEpubPreferences(s: {
  epubTheme: EpubTheme
  epubScroll: boolean
  epubFontSize: number
  epubLineHeight: number | null
  epubFontFamily: EpubFontFamily
}): EpubPreferences {
  return new EpubPreferences({
    ...THEME_COLORS[s.epubTheme],
    // multiply blend merges image backgrounds into the page color on light themes
    blendFilter: s.epubTheme !== 'NIGHT' || null,
    fontFamily: epubFontStack(s.epubFontFamily),
    scroll: s.epubScroll,
    fontSize: s.epubFontSize === 1 ? null : s.epubFontSize,
    lineHeight: s.epubLineHeight,
  })
}

/**
 * kmrs serves positions and stored progression with EPUB-internal hrefs (OEBPS/ch1.xhtml)
 * while the manifest's readingOrder uses absolute /resource/ URLs, and the navigator
 * matches hrefs by exact string — align locators to the readingOrder.
 */
export function alignLocatorHref(locator: Locator, publication: Publication): Locator {
  const item = publication.readingOrder.items.find(
    (i) => i.href === locator.href || i.href.endsWith(`/${locator.href}`),
  )
  if (!item || item.href === locator.href) return locator
  return new Locator({
    href: item.href,
    type: item.type ?? locator.type,
    title: locator.title,
    locations: locator.locations,
    text: locator.text,
  })
}

/**
 * Other clients store progressions without locations.position, which the navigator's initial
 * position requires — snap to the closest positions entry, keeping the stored progression.
 */
export function resolvePositionLocator(locator: Locator, positions: Locator[]): Locator | undefined {
  const own = locator.locations.position
  if (own !== undefined && positions.some((p) => p.locations.position === own)) return locator

  const progression = locator.locations.progression
  const sameHref = positions.filter((p) => p.href === locator.href)
  let base: Locator | undefined
  if (sameHref.length > 0) {
    base =
      progression === undefined
        ? sameHref[0]
        : sameHref.reduce((a, b) =>
            Math.abs((b.locations.progression ?? 0) - progression) <
            Math.abs((a.locations.progression ?? 0) - progression)
              ? b
              : a,
          )
  } else {
    const totalProgression = locator.locations.totalProgression
    if (totalProgression !== undefined) {
      base = positions.reduce((a, b) =>
        Math.abs((b.locations.totalProgression ?? 0) - totalProgression) <
        Math.abs((a.locations.totalProgression ?? 0) - totalProgression)
          ? b
          : a,
      )
    }
  }
  if (!base) return undefined

  const overrides: { progression?: number; totalProgression?: number } = {}
  if (progression !== undefined) overrides.progression = progression
  if (locator.locations.totalProgression !== undefined)
    overrides.totalProgression = locator.locations.totalProgression
  return base.copyWithLocations(overrides)
}

/** 1-based position number for the slider: the locator's own position, else the readingOrder match. */
export function positionOf(locator: Locator, positions: Locator[]): number {
  const own = locator.locations.position
  if (own && own >= 1) return Math.floor(own)
  const index = positions.findIndex((p) => p.href === locator.href)
  return index >= 0 ? index + 1 : 1
}

/** kmrs validates progression hrefs against EPUB-internal paths, not the absolute /resource/ URLs the navigator uses. */
export function progressionLocator(locator: Locator): unknown {
  const json = locator.serialize()
  const marker = '/resource/'
  const index = typeof json.href === 'string' ? json.href.indexOf(marker) : -1
  if (index >= 0) json.href = json.href.slice(index + marker.length)
  return json
}

export interface TocEntry {
  title?: string
  href?: string
  children?: TocEntry[]
}

/** kmrs puts toc/landmarks/pageList at the manifest top level; ts-toolkit only parses toc. */
export function manifestEntries(json: unknown, key: 'toc' | 'landmarks' | 'pageList'): TocEntry[] {
  if (!json || typeof json !== 'object') return []
  const value = (json as Record<string, unknown>)[key]
  return Array.isArray(value) ? (value as TocEntry[]) : []
}
