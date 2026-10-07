import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { useReducedMotion } from 'motion/react'
import { useTranslation } from 'react-i18next'
import { useReaderSettings } from '@/lib/store/readerSettings'
import type { SpreadPage } from '@/lib/utils/spreads'
import { useWindowKeys } from './keys'
import { usePinchZoom } from './usePinchZoom'

interface ContinuousReaderProps {
  pages: SpreadPage[]
  page: number
  onPageChange: (page: number) => void
  onToggleChrome: () => void
  onJumpPrevious: () => void
  onJumpNext: () => void
}

export function ContinuousReader({
  pages,
  page,
  onPageChange,
  onToggleChrome,
  onJumpPrevious,
  onJumpNext,
}: ContinuousReaderProps) {
  const scale = useReaderSettings((s) => s.continuousScale)
  const padding = useReaderSettings((s) => s.continuousPadding)
  const margin = useReaderSettings((s) => s.continuousMargin)
  const animations = useReaderSettings((s) => s.animations)
  const reduceMotion = useReducedMotion()
  const { t } = useTranslation('reader')

  const {
    targetRef,
    zoomed,
    touchAction,
    onPointerDown,
    onPointerMove,
    onPointerUp,
    onPointerCancel,
    consumeClick,
    reset: resetPinch,
  } = usePinchZoom({ zoomedTouchAction: 'pan-y' })
  // fit/padding changes shift the strip geometry and leave stale pan offsets behind
  useLayoutEffect(() => {
    resetPinch()
  }, [scale, padding, margin, resetPinch])

  const scrollRef = useRef<HTMLDivElement>(null)
  const currentRef = useRef(page)
  // scroll snapshot for boundary checks; refs only, the scroll path must not re-render
  const posRef = useRef({ top: 0, max: 1 })
  const lastKeyRef = useRef(0)
  const [seen, setSeen] = useState<boolean[]>(() => pages.map(() => false))

  const onPageChangeRef = useRef(onPageChange)
  useEffect(() => {
    onPageChangeRef.current = onPageChange
  })

  const markSeen = useCallback((i: number) => {
    setSeen((prev) => (prev[i] ? prev : prev.map((v, j) => (j === i ? true : v))))
  }, [])

  useEffect(() => {
    const root = scrollRef.current
    if (!root) return
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (!entry.isIntersecting) continue
          const n = Number((entry.target as HTMLElement).dataset.page)
          currentRef.current = n
          markSeen(n - 1)
          onPageChangeRef.current(n)
        }
      },
      { root, rootMargin: '-45% 0px -45% 0px' },
    )
    root.querySelectorAll('[data-page]').forEach((el) => observer.observe(el))
    return () => observer.disconnect()
  }, [pages, markSeen])

  // initial position + boundary snapshot, once placeholders have laid out
  useEffect(() => {
    const el = scrollRef.current
    if (!el) return
    if (page !== 1) el.querySelector(`[data-page="${page}"]`)?.scrollIntoView({ block: 'start' })
    posRef.current = { top: el.scrollTop, max: el.scrollHeight - el.clientHeight }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // external jumps (slider, thumbnails) scroll instantly; observer-driven changes already match
  useEffect(() => {
    if (page === currentRef.current) return
    currentRef.current = page
    scrollRef.current?.querySelector(`[data-page="${page}"]`)?.scrollIntoView({ block: 'start' })
  }, [page])

  const onScroll = () => {
    const el = scrollRef.current
    if (el) posRef.current = { top: el.scrollTop, max: el.scrollHeight - el.clientHeight }
  }

  const smooth = animations && !reduceMotion
  const scrollStep = (dir: 1 | -1) => {
    const el = scrollRef.current
    if (!el) return
    el.scrollBy({ top: dir * el.clientHeight * 0.95, behavior: smooth ? 'smooth' : 'auto' })
  }
  const prev = () => {
    if (posRef.current.top > 1) scrollStep(-1)
    else onJumpPrevious()
  }
  const next = () => {
    if (posRef.current.top < posRef.current.max - 1) scrollStep(1)
    else onJumpNext()
  }

  useWindowKeys((e) => {
    const forward = e.key === ' ' || e.key === 'PageDown' || e.key === 'ArrowDown'
    const back = e.key === 'PageUp' || e.key === 'ArrowUp'
    if (!forward && !back) return
    e.preventDefault()
    const now = Date.now()
    if (now - lastKeyRef.current < 500) return
    lastKeyRef.current = now
    if (forward) next()
    else prev()
  })

  // click regions instead of overlay zones: overlays would swallow wheel scrolling
  const onClick = (e: React.MouseEvent<HTMLDivElement>) => {
    if (consumeClick()) return
    // while zoomed a tap only toggles chrome; a scroll jump would dump the zoom
    if (zoomed) {
      onToggleChrome()
      return
    }
    const rect = e.currentTarget.getBoundingClientRect()
    const ratio = (e.clientY - rect.top) / rect.height
    if (ratio < 0.25) prev()
    else if (ratio > 0.75) next()
    else onToggleChrome()
  }

  return (
    <div
      className="h-full overflow-hidden"
      style={{ touchAction }}
      onClick={onClick}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerCancel}
    >
      <div
        ref={targetRef}
        className="h-full"
        style={{ transformOrigin: '0 0', willChange: zoomed ? 'transform' : undefined }}
      >
        <div ref={scrollRef} onScroll={onScroll} className="h-full overflow-y-auto overscroll-none no-scrollbar">
          {pages.map((p, i) => {
            const load = i === 0 || seen[i] || Math.abs(i + 1 - page) <= 2
            return (
              <div
                key={p.number}
                data-page={p.number}
                className="mx-auto"
                style={{
                  width: scale === 'WIDTH' ? `${100 - padding * 2}%` : p.width ? p.width : undefined,
                  marginTop: i === 0 ? 0 : margin,
                  aspectRatio: p.width && p.height ? `${p.width} / ${p.height}` : undefined,
                }}
              >
                {load && (
                  <img src={p.url} alt={t('pageAlt', { page: p.number })} draggable={false} className="block h-auto w-full" />
                )}
              </div>
            )
          })}
        </div>
      </div>
    </div>
  )
}
