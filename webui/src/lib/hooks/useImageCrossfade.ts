import { useState, type TransitionEvent } from 'react'

/**
 * Crossfade state for <img> src swaps: a busted thumbnail URL would otherwise
 * snap to the new image, so the last loaded image stays as a base layer under
 * the replacement until its fade-in ends.
 */
export function useImageCrossfade(src: string) {
  const [state, setState] = useState({ src, base: null as string | null, loaded: false, error: false })
  if (state.src !== src) {
    setState({
      src,
      // a swap before any load keeps the older base instead of flashing empty
      base: state.loaded && !state.error ? state.src : state.base,
      loaded: false,
      error: false,
    })
  }
  return {
    base: state.base,
    loaded: state.loaded,
    error: state.error,
    onLoad: () => setState((s) => ({ ...s, loaded: true })),
    onError: () => setState((s) => ({ ...s, base: null, error: true })),
    onTransitionEnd: (e: TransitionEvent) => {
      if (e.propertyName !== 'opacity') return
      setState((s) => (s.loaded && s.base ? { ...s, base: null } : s))
    },
  }
}
