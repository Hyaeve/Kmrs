import { useLayoutEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils/cn'

// literal classes: the bundler only generates utilities it can see verbatim
const CLAMPS = {
  2: 'line-clamp-2',
  3: 'line-clamp-3',
  4: 'line-clamp-4',
} as const

/** Long text clamped to a few lines with a More/Less toggle, shown only when it actually overflows. */
export function ClampedText({ text, lines = 4, className }: { text: string; lines?: keyof typeof CLAMPS; className?: string }) {
  const { t } = useTranslation()
  const [expanded, setExpanded] = useState(false)
  const [overflows, setOverflows] = useState(false)
  const ref = useRef<HTMLParagraphElement>(null)

  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    setOverflows(el.scrollHeight > el.clientHeight + 1)
  }, [text])

  return (
    <>
      <p ref={ref} className={cn('whitespace-pre-line', !expanded && CLAMPS[lines], className)}>
        {text}
      </p>
      {(overflows || expanded) && (
        <button
          type="button"
          onClick={() => setExpanded((v) => !v)}
          className="mt-1 cursor-pointer text-sm text-accent-strong hover:underline"
        >
          {expanded ? t('action.showLess') : t('action.showMore')}
        </button>
      )}
    </>
  )
}
