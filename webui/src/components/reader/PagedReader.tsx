import { useCallback, useEffect, useLayoutEffect, useMemo, useRef } from 'react'
import { useReducedMotion } from 'motion/react'
import { useTranslation } from 'react-i18next'
import type { ReadingDirection } from '@/lib/api/types'
import { useReaderSettings, type ScaleType } from '@/lib/store/readerSettings'
import { buildSpreads, type SpreadPage } from '@/lib/utils/spreads'
import { cn } from '@/lib/utils/cn'
import { useWindowKeys } from './keys'
import { usePinchZoom } from './usePinchZoom'

interface PagedReaderProps {
  pages: SpreadPage[]
  page: number
  direction: ReadingDirection
  onPageChange: (page: number) => void
  onToggleChrome: () => void
  onJumpPrevious: () => void
  onJumpNext: () => void
}

/** The page a spread reports back: the visually last one (right page in LTR, left page in RTL). */
function spreadCurrentPage(spread: SpreadPage[]): number {
  return spread.length === 2 && !spread[1].blank ? spread[1].number : spread[0].number
}

function imgClass(scale: ScaleType, double: boolean): string {
  switch (scale) {
    case 'WIDTH':
      return cn('self-start', double ? 'w-1/2' : 'w-full')
    case 'WIDTH_SHRINK_ONLY':
      return cn('self-start', double ? 'max-w-1/2' : 'max-w-full')
    case 'HEIGHT':
      return 'h-dvh w-auto'
    case 'ORIGINAL':
      return 'h-auto w-auto max-w-none'
    default:
      return cn('max-h-dvh object-contain', double ? 'max-w-1/2' : 'max-w-full')
  }
}

export function PagedReader({
  pages,
  page,
  direction,
  onPageChange,
  onToggleChrome,
  onJumpPrevious,
  onJumpNext,
}: PagedReaderProps) {
  const scale = useReaderSettings((s) => s.scale)
  const pageLayout = useReaderSettings((s) => s.pageLayout)
  const animations = useReaderSettings((s) => s.animations)
  const swipe = useReaderSettings((s) => s.swipe)
  const reduceMotion = useReducedMotion()
  const { t } = useTranslation('reader')

  const spreads = useMemo(() => buildSpreads(pages, pageLayout), [pages, pageLayout])
  const pageToSpread = useMemo(() => {
    const map: number[] = []
    spreads.forEach((spread, i) => spread.forEach((p) => !p.blank && (map[p.number] = i)))
    return map
  }, [spreads])

  const flip = direction === 'RIGHT_TO_LEFT'
  const vertical = direction === 'VERTICAL'
  const spreadIndex = pageToSpread[page] ?? 0
  const animate = animations && !reduceMotion
  // fit modes stay inside the viewport; the rest need a scrollable slide
  const scrollable = scale === 'WIDTH' || scale === 'WIDTH_SHRINK_ONLY' || scale === 'ORIGINAL'
  // hugging the content keeps horizontal overflow reachable by scroll while small content stays centered
  const hugContent = scale === 'ORIGINAL' || scale === 'HEIGHT'

  const {
    targetRef,
    zoomed,
    touchAction,
    onPointerDown: pinchPointerDown,
    onPointerMove,
    onPointerUp: pinchPointerUp,
    onPointerCancel: pinchPointerCancel,
    gestureConsumed,
    consumeClick,
    reset: resetPinch,
  } = usePinchZoom({ zoomedTouchAction: 'none' })
  // layout shifts (page turn, fit mode, direction) leave stale pan offsets behind
  useLayoutEffect(() => {
    resetPinch()
  }, [spreadIndex, scale, pageLayout, direction, resetPinch])

  const goSpread = useCallback((i: number) => onPageChange(spreadCurrentPage(spreads[i])), [spreads, onPageChange])
  const prev = useCallback(() => {
    if (spreadIndex > 0) goSpread(spreadIndex - 1)
    else onJumpPrevious()
  }, [spreadIndex, goSpread, onJumpPrevious])
  const next = useCallback(() => {
    if (spreadIndex < spreads.length - 1) goSpread(spreadIndex + 1)
    else onJumpNext()
  }, [spreadIndex, spreads.length, goSpread, onJumpNext])
  const turnLeft = useCallback(() => {
    if (!vertical) (flip ? next : prev)()
  }, [vertical, flip, next, prev])
  const turnRight = useCallback(() => {
    if (!vertical) (flip ? prev : next)()
  }, [vertical, flip, next, prev])

  useWindowKeys((e) => {
    switch (e.key) {
      case 'ArrowLeft':
        turnLeft()
        break
      case 'ArrowRight':
        turnRight()
        break
      case 'ArrowUp':
        if (vertical) prev()
        break
      case 'ArrowDown':
        if (vertical) next()
        break
    }
  })

  // fetch two spreads ahead of the ±2 render window
  useEffect(() => {
    for (const i of [spreadIndex + 3, spreadIndex + 4]) {
      spreads[i]?.forEach((p) => {
        if (!p.blank) {
          const img = new Image()
          img.src = p.url
        }
      })
    }
  }, [spreadIndex, spreads])

  // a swipe consumes the click that the browser fires after pointerup
  const pointerRef = useRef<{ x: number; y: number } | null>(null)
  const swipedRef = useRef(false)
  const onPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    pinchPointerDown(e)
    if (!swipe || !e.isPrimary) return
    pointerRef.current = { x: e.clientX, y: e.clientY }
  }
  const onPointerUp = (e: React.PointerEvent<HTMLDivElement>) => {
    pinchPointerUp(e)
    const start = pointerRef.current
    pointerRef.current = null
    if (!start) return
    if (gestureConsumed()) return
    const dx = e.clientX - start.x
    const dy = e.clientY - start.y
    if (vertical) {
      if (Math.abs(dy) < 48 || Math.abs(dy) < Math.abs(dx)) return
      swipedRef.current = true
      ;(dy < 0 ? next : prev)()
    } else {
      if (Math.abs(dx) < 48 || Math.abs(dx) < Math.abs(dy)) return
      swipedRef.current = true
      ;(dx < 0 ? turnRight : turnLeft)()
    }
  }
  const onPointerCancel = (e: React.PointerEvent<HTMLDivElement>) => {
    pinchPointerCancel(e)
    pointerRef.current = null
  }

  // click regions instead of overlay zones: overlays would swallow wheel scrolling
  const onClick = (e: React.MouseEvent<HTMLDivElement>) => {
    if (swipedRef.current) {
      swipedRef.current = false
      return
    }
    if (consumeClick()) return
    // while zoomed a tap only toggles chrome; a page turn would dump the zoom
    if (zoomed) {
      onToggleChrome()
      return
    }
    const rect = e.currentTarget.getBoundingClientRect()
    const ratio = vertical ? (e.clientY - rect.top) / rect.height : (e.clientX - rect.left) / rect.width
    if (ratio < 0.25) (vertical ? prev : turnLeft)()
    else if (ratio > 0.75) (vertical ? next : turnRight)()
    else onToggleChrome()
  }

  const transform = vertical
    ? `translateY(${-spreadIndex * 100}%)`
    : `translateX(${(flip ? 1 : -1) * spreadIndex * 100}%)`

  return (
    <div
      className="h-full w-full overflow-hidden"
      style={{ touchAction }}
      onClick={onClick}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerCancel}
    >
      <div
        ref={targetRef}
        className="h-full w-full"
        style={{ transformOrigin: '0 0', willChange: zoomed ? 'transform' : undefined }}
      >
        <div
          className={cn(
            'flex h-full w-full',
            vertical ? 'flex-col' : flip && 'flex-row-reverse',
            animate && 'transition-transform duration-300 ease-out-expo',
          )}
          style={{ transform }}
        >
          {spreads.map((spread, i) => {
            const double = spread.length > 1
            const near = Math.abs(i - spreadIndex) <= 2
            return (
              <div key={i} className="flex h-full w-full shrink-0 overflow-auto no-scrollbar">
                <div
                  className={cn(
                    'm-auto flex justify-center',
                    flip && !vertical && 'flex-row-reverse',
                    hugContent ? 'w-max min-w-full' : 'w-full',
                    scrollable ? 'items-start' : 'items-center',
                  )}
                >
                  {spread.map((p, j) => {
                    const cls = imgClass(scale, double)
                    const dim = p.width && p.height ? { aspectRatio: `${p.width} / ${p.height}` } : undefined
                    return near || p.blank ? (
                      <img
                        key={p.number || `blank-${j}`}
                        src={p.url}
                        alt={p.blank ? '' : t('pageAlt', { page: p.number })}
                        draggable={false}
                        className={cls}
                        style={dim}
                      />
                    ) : (
                      <div key={p.number} aria-hidden className={cls} style={dim} />
                    )
                  })}
                </div>
              </div>
            )
          })}
        </div>
      </div>
    </div>
  )
}
