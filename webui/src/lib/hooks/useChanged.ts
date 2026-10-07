import { useState } from 'react'

/** True on the render where any dep differs from the previous render (never on mount),
    so state derived from props can adjust during render instead of in an effect. */
export function useChanged(deps: readonly unknown[]): boolean {
  const [prev, setPrev] = useState(deps)
  if (prev.length === deps.length && prev.every((d, i) => Object.is(d, deps[i]))) return false
  setPrev(deps)
  return true
}
