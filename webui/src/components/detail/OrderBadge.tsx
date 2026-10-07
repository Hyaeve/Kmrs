/** Position marker for ordered collection/read list cards, overlaid on the cover. */
export function OrderBadge({ index }: { index: number }) {
  return (
    <span className="pointer-events-none absolute top-1.5 left-1.5 rounded-full bg-black/60 px-1.5 font-mono text-[11px] text-white">
      {index}
    </span>
  )
}
