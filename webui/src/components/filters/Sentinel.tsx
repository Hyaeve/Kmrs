import { useEffect, useRef } from 'react'

interface SentinelProps {
  /** observing is paused while false (e.g. no next page or placeholder data shown) */
  active: boolean
  onIntersect: () => void
}

export function Sentinel({ active, onIntersect }: SentinelProps) {
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const el = ref.current
    if (!el || !active) return
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) onIntersect()
      },
      { rootMargin: '800px 0px' },
    )
    observer.observe(el)
    return () => observer.disconnect()
  }, [active, onIntersect])

  return <div ref={ref} aria-hidden className="h-px w-full" />
}
