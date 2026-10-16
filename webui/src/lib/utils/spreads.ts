import type { PageDto } from '@/lib/api/types'

export type PagedReaderLayout = 'SINGLE_PAGE' | 'DOUBLE_PAGES' | 'DOUBLE_NO_COVER'

export interface SpreadPage extends Partial<PageDto> {
  number: number
  url: string
  blank?: boolean
}

export const isLandscape = (p: { width?: number; height?: number }): boolean =>
  !!p.width && !!p.height && p.width > p.height

/**
 * Port of komga-webui's buildSpreads: cover page pairs with a blank on its
 * outer side, landscape pages always get a solo spread.
 */
export function buildSpreads(pages: SpreadPage[], layout: PagedReaderLayout): SpreadPage[][] {
  if (pages.length === 0) return []
  if (layout === 'SINGLE_PAGE') return pages.map((p) => [p])

  const spreads: SpreadPage[][] = []
  const queue = [...pages]
  let lastSpread: SpreadPage[] | undefined

  if (layout === 'DOUBLE_PAGES') {
    const first = queue.shift()!
    spreads.push(isLandscape(first) ? [first] : [blankOf(first), first])
    if (queue.length > 0) {
      const last = queue.pop()!
      lastSpread = isLandscape(last) ? [last] : [last, blankOf(last)]
    }
  }

  while (queue.length > 0) {
    const p = queue.shift()!
    if (isLandscape(p)) {
      spreads.push([p])
    } else if (queue.length > 0) {
      const p2 = queue.shift()!
      if (isLandscape(p2)) {
        spreads.push([p, blankOf(p)])
        spreads.push([p2])
      } else {
        spreads.push([p, p2])
      }
    } else {
      spreads.push([p, blankOf(p)])
    }
  }

  if (lastSpread) spreads.push(lastSpread)
  return spreads
}

function blankOf(page: SpreadPage): SpreadPage {
  return {
    number: 0,
    url: transparentDataUrl(page.width || 20, page.height || 30),
    width: page.width,
    height: page.height,
    blank: true,
  }
}

function transparentDataUrl(w: number, h: number): string {
  const canvas = document.createElement('canvas')
  canvas.width = w
  canvas.height = h
  const ctx = canvas.getContext('2d')
  if (ctx) {
    ctx.fillStyle = 'rgb(0,0,0,0)'
    ctx.fillRect(0, 0, w, h)
  }
  return canvas.toDataURL()
}
