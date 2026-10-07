import { useCallback, useMemo, useRef, useState } from 'react'

export interface Selection {
  /** selected ids, in insertion order */
  ids: string[]
  size: number
  /** true once at least one item is selected; drives card-wide toggle behavior */
  active: boolean
  has: (id: string) => boolean
  toggle: (id: string) => void
  /** shift-click: applies to the inclusive range between the last toggled id and `id` within
      orderedIds — deselects the range when `id` is selected, selects it otherwise */
  toggleRange: (id: string, orderedIds: string[]) => void
  selectAll: (ids: string[]) => void
  clear: () => void
}

export function useSelection(): Selection {
  const [ids, setIds] = useState<string[]>([])
  const anchorRef = useRef<string | null>(null)

  const toggle = useCallback((id: string) => {
    anchorRef.current = id
    setIds((prev) => (prev.includes(id) ? prev.filter((v) => v !== id) : [...prev, id]))
  }, [])

  const toggleRange = useCallback((id: string, orderedIds: string[]) => {
    const anchor = anchorRef.current
    anchorRef.current = id
    const from = anchor === null ? -1 : orderedIds.indexOf(anchor)
    const to = orderedIds.indexOf(id)
    // without an anchor inside the loaded list there is no range to extend
    if (from === -1 || to === -1) {
      setIds((prev) => (prev.includes(id) ? prev.filter((v) => v !== id) : [...prev, id]))
      return
    }
    const [lo, hi] = from < to ? [from, to] : [to, from]
    const range = orderedIds.slice(lo, hi + 1)
    setIds((prev) => {
      if (!prev.includes(id)) return [...prev, ...range.filter((v) => !prev.includes(v))]
      const drop = new Set(range)
      return prev.filter((v) => !drop.has(v))
    })
  }, [])

  const selectAll = useCallback((all: string[]) => {
    anchorRef.current = null
    setIds(all)
  }, [])
  // keep the same state when already empty so filter-change effects don't re-render
  const clear = useCallback(() => {
    anchorRef.current = null
    setIds((prev) => (prev.length ? [] : prev))
  }, [])
  const has = useCallback((id: string) => ids.includes(id), [ids])

  return useMemo(
    () => ({ ids, size: ids.length, active: ids.length > 0, has, toggle, toggleRange, selectAll, clear }),
    [ids, has, toggle, toggleRange, selectAll, clear],
  )
}

export interface CardSelection {
  /** selection mode is on: checkbox always visible, card click toggles */
  active: boolean
  selected: boolean
  /** shift extends the toggle to the range between the last toggled card and this one */
  onToggle: (shift?: boolean) => void
}

export function cardSelection(sel: Selection, id: string, orderedIds: string[]): CardSelection {
  return {
    active: sel.active,
    selected: sel.has(id),
    onToggle: (shift) => (shift ? sel.toggleRange(id, orderedIds) : sel.toggle(id)),
  }
}
