import { useEffect, useRef } from 'react'

// dialogs/menus/inputs own their full keyboard behavior
const ALWAYS_CONSUMERS = 'input, textarea, select, [contenteditable="true"], [role="dialog"], [role="menu"]'
const SLIDER_KEYS = new Set(['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End', 'PageUp', 'PageDown'])

/** True when the focused control already uses this key, so reader shortcuts must not also fire. */
export function isInteractiveKeyTarget(target: EventTarget | null, key: string): boolean {
  if (!(target instanceof HTMLElement)) return false
  if (target.closest(ALWAYS_CONSUMERS)) return true
  if (SLIDER_KEYS.has(key) && target.closest('[role="slider"]')) return true
  if ((key === ' ' || key === 'Enter') && target.closest('[role="switch"], button, a[href]')) return true
  return false
}

/** Window-level keydown subscription that always sees the latest handler. */
export function useWindowKeys(handler: (e: KeyboardEvent) => void): void {
  const ref = useRef(handler)
  useEffect(() => {
    ref.current = handler
  })
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey || e.altKey || e.shiftKey || e.metaKey) return
      if (isInteractiveKeyTarget(e.target, e.key)) return
      ref.current(e)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])
}
