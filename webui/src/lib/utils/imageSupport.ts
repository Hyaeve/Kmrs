// Data-URL probes for formats kmrs may serve but older browsers cannot decode.
// Unsupported formats get ?convert=jpeg on page URLs.

const PROBES: Record<string, string> = {
  webp: 'data:image/webp;base64,UklGRiIAAABXRUJQVlA4IBYAAAAwAQCdASoBAAEADsD+JaQAA3AAAAAA',
  avif:
    'data:image/avif;base64,AAAAIGZ0eXBhdmlmAAAAAGF2aWZtaWYxbWlhZk1BMUIAAADybWV0YQAAAAAAAAAoaGRscgAAAAAAAAAAcGljdAAAAAAAAAAAAAAAAGxpYmF2aWYAAAAADnBpdG0AAAAAAAEAAAAeaWxvYwAAAABEAAABAAEAAAABAAABGgAAAB0AAAAoaWluZgAAAAAAAQAAABppbmZlAgAAAAABAABhdjAxQ29sb3IAAAAAamlwcnAAAABLaXBjbwAAABRpc3BlAAAAAAAAAAIAAAACAAAAEHBpeGkAAAAAAwgICAAAAAxhdjFDgQ0MAAAAABNjb2xybmNseAACAAIAAYAAAAAXaXBtYQAAAAAAAAABAAEEAQKDBAAAACVtZGF0EgAKCBgANogQEAwgMg8f8D///8WfhwB8+ErK42A=',
  jxl: 'data:image/jxl;base64,/woIELASCAgQAFwASxLFgkWAHL0xqnCBCV0qDp901Te/5QM=',
}

function probe(src: string): Promise<boolean> {
  return new Promise((resolve) => {
    const img = new Image()
    img.onload = () => resolve(img.width > 0 && img.height > 0)
    img.onerror = () => resolve(false)
    img.src = src
  })
}

let cache: Record<string, boolean> | null = null

export async function supportedImageFormats(): Promise<Record<string, boolean>> {
  if (cache) return cache
  const entries = await Promise.all(
    Object.entries(PROBES).map(async ([key, src]) => [key, await probe(src)] as const),
  )
  cache = Object.fromEntries(entries)
  return cache
}

/** True when the media type needs a server-side transcode for this browser. */
export function needsConvert(mediaType: string, supported: Record<string, boolean>): boolean {
  const mt = mediaType.toLowerCase()
  if (mt.includes('webp')) return !supported.webp
  if (mt.includes('avif')) return !supported.avif
  if (mt.includes('jxl') || mt.includes('jpeg-xl')) return !supported.jxl
  if (mt.includes('heif') || mt.includes('heic')) return true
  if (mt.includes('jp2') || mt.includes('jpeg2000')) return true
  return false
}
