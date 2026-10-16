import { useEffect, useState } from 'react'

const SAMPLE_SIZE = 8

// keyed by full url, so a bust-param change (poster refresh) re-extracts
const cache = new Map<string, string>()

/** The cover's average color, clamped so white card text stays readable without the
    card going pitch black. */
function extractTint(img: HTMLImageElement): string | null {
  const ctx = document.createElement('canvas').getContext('2d', { willReadFrequently: true })
  if (!ctx) return null
  ctx.canvas.width = SAMPLE_SIZE
  ctx.canvas.height = SAMPLE_SIZE
  ctx.drawImage(img, 0, 0, SAMPLE_SIZE, SAMPLE_SIZE)
  let data: Uint8ClampedArray
  try {
    data = ctx.getImageData(0, 0, SAMPLE_SIZE, SAMPLE_SIZE).data
  } catch {
    return null
  }

  let r = 0
  let g = 0
  let b = 0
  for (let i = 0; i < data.length; i += 4) {
    r += data[i]
    g += data[i + 1]
    b += data[i + 2]
  }
  const n = SAMPLE_SIZE * SAMPLE_SIZE * 255
  r /= n
  g /= n
  b /= n

  const max = Math.max(r, g, b)
  const min = Math.min(r, g, b)
  const delta = max - min
  let h = 0
  if (delta > 0) {
    if (max === r) h = ((g - b) / delta) % 6
    else if (max === g) h = (b - r) / delta + 2
    else h = (r - g) / delta + 4
    h /= 6
    if (h < 0) h += 1
  }
  const s = max === 0 ? 0 : Math.min(delta / max, 0.8)
  const v = Math.min(Math.max(max, 0.2), 0.45)

  const i = Math.floor(h * 6)
  const f = h * 6 - i
  const p = v * (1 - s)
  const q = v * (1 - f * s)
  const t = v * (1 - (1 - f) * s)
  const [rr, gg, bb] = [
    [v, t, p],
    [q, v, p],
    [p, v, t],
    [p, q, v],
    [t, p, v],
    [v, p, q],
  ][i % 6]
  return `rgb(${Math.round(rr * 255)} ${Math.round(gg * 255)} ${Math.round(bb * 255)})`
}

/** null while unresolved (or extraction failed); callers fall back to the default card fill */
export function useCoverTint(src: string): string | null {
  const [tint, setTint] = useState<string | null>(() => cache.get(src) ?? null)

  const [prevSrc, setPrevSrc] = useState(src)
  if (prevSrc !== src) {
    setPrevSrc(src)
    const hit = cache.get(src)
    if (hit) setTint(hit)
  }

  useEffect(() => {
    if (cache.get(src)) return
    let cancelled = false
    const img = new Image()
    img.onload = () => {
      const color = extractTint(img)
      // only cache successes: load failures are usually transient, so a remount retries
      if (color) cache.set(src, color)
      if (!cancelled) setTint(color)
    }
    img.onerror = () => {
      if (!cancelled) setTint(null)
    }
    img.src = src
    return () => {
      cancelled = true
    }
  }, [src])

  return tint
}
