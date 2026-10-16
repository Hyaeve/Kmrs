import { useCallback, useRef, useState } from 'react'

const MAX_SCALE = 5
/** accumulated drag distance before a gesture counts as a pan; below it the trailing click stays a tap */
const PAN_SLOP = 8
/** a gesture ending below this scale snaps back to 1 so no residual zoom lingers */
const SNAP_BACK = 1.08

interface PinchZoomOptions {
  /** paged reader pans purely in JS ('none'); webtoon keeps native vertical scroll while zoomed ('pan-y') */
  zoomedTouchAction: 'none' | 'pan-y'
}

interface Point {
  x: number
  y: number
}

/**
 * Two-finger pinch zoom plus one-finger pan for the readers. Frames are written
 * straight to the target's style so gestures never re-render; React state only
 * tracks whether zoom is engaged, which gates swipe/click turning and touch-action.
 */
export function usePinchZoom({ zoomedTouchAction }: PinchZoomOptions) {
  const targetRef = useRef<HTMLDivElement>(null)
  const [zoomed, setZoomed] = useState(false)

  const pointers = useRef(new Map<number, Point>())
  const mode = useRef<'idle' | 'pinch' | 'pan'>('idle')
  const prevDist = useRef(0)
  const prevMid = useRef<Point>({ x: 0, y: 0 })
  const last = useRef<Point>({ x: 0, y: 0 })
  const panTravel = useRef(0)
  const transform = useRef({ s: 1, tx: 0, ty: 0 })
  // pointer positions are stored relative to the gesture element, which is also the transform space
  const origin = useRef<Point>({ x: 0, y: 0 })
  const bounds = useRef({ w: 0, h: 0 })
  // a gesture that grew a second finger or panned must not also turn the page or fire its trailing click
  const dirty = useRef(false)
  const suppressClick = useRef(false)

  const write = useCallback(() => {
    const el = targetRef.current
    if (!el) return
    const { s, tx, ty } = transform.current
    el.style.transform = s === 1 ? '' : `translate(${tx}px, ${ty}px) scale(${s})`
  }, [])

  const apply = useCallback(
    (s: number, tx: number, ty: number) => {
      const { w, h } = bounds.current
      const t = transform.current
      t.s = s
      t.tx = Math.min(0, Math.max(w * (1 - s), tx))
      t.ty = Math.min(0, Math.max(h * (1 - s), ty))
      write()
      setZoomed(s > 1)
    },
    [write],
  )

  const reset = useCallback(() => {
    pointers.current.clear()
    mode.current = 'idle'
    prevDist.current = 0
    panTravel.current = 0
    dirty.current = false
    suppressClick.current = false
    transform.current = { s: 1, tx: 0, ty: 0 }
    write()
    setZoomed(false)
  }, [write])

  const onPointerDown = useCallback((e: React.PointerEvent<HTMLDivElement>) => {
    e.currentTarget.setPointerCapture(e.pointerId)
    if (pointers.current.size === 0) {
      const rect = e.currentTarget.getBoundingClientRect()
      origin.current = { x: rect.left, y: rect.top }
      bounds.current = { w: rect.width, h: rect.height }
      mode.current = transform.current.s > 1 ? 'pan' : 'idle'
      last.current = { x: e.clientX - rect.left, y: e.clientY - rect.top }
      panTravel.current = 0
      dirty.current = false
      suppressClick.current = false
    } else if (pointers.current.size === 1) {
      mode.current = 'pinch'
      dirty.current = true
      prevDist.current = 0
    }
    pointers.current.set(e.pointerId, { x: e.clientX - origin.current.x, y: e.clientY - origin.current.y })
  }, [])

  const onPointerMove = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      const p = pointers.current.get(e.pointerId)
      if (!p) return
      p.x = e.clientX - origin.current.x
      p.y = e.clientY - origin.current.y
      if (mode.current === 'pinch') {
        const [a, b] = [...pointers.current.values()]
        if (!a || !b) return
        const dist = Math.hypot(a.x - b.x, a.y - b.y)
        const mid = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 }
        if (prevDist.current > 0 && dist > 0) {
          const t = transform.current
          const s = Math.min(MAX_SCALE, Math.max(1, t.s * (dist / prevDist.current)))
          const k = s / t.s
          // pan with the drifting midpoint first, then scale around it, so the content stays under the fingers
          const tx = mid.x - (mid.x - t.tx - (mid.x - prevMid.current.x)) * k
          const ty = mid.y - (mid.y - t.ty - (mid.y - prevMid.current.y)) * k
          apply(s, tx, ty)
        }
        prevDist.current = dist
        prevMid.current = mid
      } else if (mode.current === 'pan') {
        const dx = p.x - last.current.x
        const dy = p.y - last.current.y
        last.current = { x: p.x, y: p.y }
        panTravel.current += Math.abs(dx) + Math.abs(dy)
        const t = transform.current
        apply(t.s, t.tx + dx, t.ty + dy)
      }
    },
    [apply],
  )

  const endPointer = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      if (!pointers.current.delete(e.pointerId)) return
      if (pointers.current.size >= 2) {
        prevDist.current = 0
        return
      }
      if (pointers.current.size === 1) {
        // second finger lifted: the remaining one glides into a pan
        const [remaining] = pointers.current.values()
        last.current = { x: remaining.x, y: remaining.y }
        prevDist.current = 0
        mode.current = transform.current.s > 1 ? 'pan' : 'idle'
        return
      }
      const t = transform.current
      if (t.s > 1 && t.s < SNAP_BACK) apply(1, 0, 0)
      if (dirty.current || panTravel.current > PAN_SLOP) suppressClick.current = true
      mode.current = 'idle'
    },
    [apply],
  )

  /** true while a pinch/pan gesture is active or just finished, so swipe turning must not fire */
  const gestureConsumed = useCallback(
    () => dirty.current || panTravel.current > PAN_SLOP || transform.current.s > 1,
    [],
  )

  /** swallow the click the browser fires right after a pinch/pan */
  const consumeClick = useCallback(() => {
    const v = suppressClick.current
    suppressClick.current = false
    return v
  }, [])

  return {
    targetRef,
    zoomed,
    touchAction: zoomed ? zoomedTouchAction : ('pan-x pan-y' as const),
    onPointerDown,
    onPointerMove,
    onPointerUp: endPointer,
    onPointerCancel: endPointer,
    gestureConsumed,
    consumeClick,
    reset,
  }
}
