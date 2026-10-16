import { useLayoutEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils/cn'
import { FadeKey } from '@/components/ui/FadeKey'

/** Long text clamped to 4 lines with a More/Less toggle, shown only when it actually overflows. */
export function Summary({ text, className }: { text: string; className?: string }) {
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
    <FadeKey id={text} className={className}>
      <p ref={ref} className={cn('max-w-3xl leading-relaxed text-ink-2', !expanded && 'line-clamp-4')}>
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
    </FadeKey>
  )
}
