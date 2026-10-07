import { Children, useEffect, useRef, useState, type ReactNode, type UIEvent } from 'react'
import { Link } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { CaretLeft, CaretRight } from '@phosphor-icons/react'
import { motion } from 'motion/react'
import { cn } from '@/lib/utils/cn'
import { IconButton } from '@/components/ui/IconButton'

interface HorizontalRowProps {
  title: string
  /** "see all" target; renders a chevron link next to the title */
  to?: string
  /** start collapsed; the title becomes a toggle, for secondary info not worth default vertical space */
  collapsible?: boolean
  children: ReactNode
  className?: string
  /** fired when scrolled past 95% (dashboard infinite load) */
  onEndReached?: () => void
}

export function HorizontalRow({ title, to, collapsible, children, className, onEndReached }: HorizontalRowProps) {
  const { t } = useTranslation('media')
  const [open, setOpen] = useState(false)
  const expanded = !collapsible || open
  const count = Children.count(children)
  const ref = useRef<HTMLDivElement>(null)
  const [canLeft, setCanLeft] = useState(false)
  const [canRight, setCanRight] = useState(false)
  const endFired = useRef(false)

  const update = () => {
    const el = ref.current
    if (!el) return
    setCanLeft(el.scrollLeft > 4)
    setCanRight(el.scrollLeft + el.clientWidth < el.scrollWidth - 4)
  }

  useEffect(() => {
    update()
    const el = ref.current
    if (!el) return
    const ro = new ResizeObserver(update)
    ro.observe(el)
    return () => ro.disconnect()
    // children length changes shift scrollWidth; re-measure
  }, [children])

  const scrollBy = (dir: 1 | -1) => {
    const el = ref.current
    if (!el) return
    el.scrollBy({ left: dir * (el.clientWidth - 100), behavior: 'smooth' })
  }

  const onScroll = (e: UIEvent<HTMLDivElement>) => {
    update()
    const el = e.currentTarget
    if (onEndReached && !endFired.current && el.scrollLeft + el.clientWidth >= el.scrollWidth * 0.95) {
      endFired.current = true
      onEndReached()
    }
    if (el.scrollLeft + el.clientWidth < el.scrollWidth * 0.8) endFired.current = false
  }

  return (
    <motion.section
      className={cn('group/row', className)}
      initial={{ opacity: 0, y: 14 }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once: true, margin: '-40px' }}
      transition={{ duration: 0.5, ease: [0.16, 1, 0.3, 1] }}
    >
      <div className={cn('relative flex items-center gap-4', expanded && 'mb-4')}>
        {collapsible ? (
          <button
            type="button"
            onClick={() => setOpen((v) => !v)}
            aria-expanded={open}
            className="group/title flex cursor-pointer items-center gap-1.5 outline-none"
          >
            <CaretRight
              className={cn(
                'size-4 text-ink-3 transition-transform duration-150 group-hover/title:text-accent',
                open && 'rotate-90',
              )}
            />
            <h2 className="font-display text-[22px] leading-none font-semibold tracking-tight text-ink transition-colors group-hover/title:text-accent-strong">
              {title}
            </h2>
            <span className="text-sm text-ink-3">({count})</span>
          </button>
        ) : to ? (
          <Link to={to} className="group/title flex items-center gap-1.5 outline-none">
            <h2 className="font-display text-[22px] leading-none font-semibold tracking-tight text-ink transition-colors group-hover/title:text-accent-strong">
              {title}
            </h2>
            <CaretRight className="size-4 text-ink-3 transition-all group-hover/title:translate-x-0.5 group-hover/title:text-accent" />
          </Link>
        ) : (
          <h2 className="font-display text-[22px] leading-none font-semibold tracking-tight text-ink">{title}</h2>
        )}
        {expanded && (
          <div className="absolute top-1/2 right-0 hidden -translate-y-1/2 gap-1 opacity-0 transition-opacity group-hover/row:opacity-100 md:flex">
            <IconButton label={t('row.scrollLeft')} onClick={() => scrollBy(-1)} disabled={!canLeft}>
              <CaretLeft className="size-4" />
            </IconButton>
            <IconButton label={t('row.scrollRight')} onClick={() => scrollBy(1)} disabled={!canRight}>
              <CaretRight className="size-4" />
            </IconButton>
          </div>
        )}
      </div>
      {expanded && (
        <div
          ref={ref}
          onScroll={onScroll}
          // overflow-x makes overflow-y compute to auto, letting diagonal trackpad swipes rubber-band
          // the row vertically; hidden clips the same but is not user-scrollable, so vertical swipes
          // still chain to the page (overscroll-behavior would swallow them)
          // negative margins cancel the padding exactly (like -mx-2/px-2) so the title
          // margin and section gap measure true: pt-3.5 is headroom for the hover shadow
          className="no-scrollbar -mx-2 -mt-3.5 -mb-2 flex snap-x gap-4 overflow-x-auto overflow-y-hidden px-2 pt-3.5 pb-2"
        >
          {children}
        </div>
      )}
    </motion.section>
  )
}
