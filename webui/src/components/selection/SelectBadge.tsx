import { useTranslation } from 'react-i18next'
import { Check } from '@phosphor-icons/react'
import { cn } from '@/lib/utils/cn'
import type { CardSelection } from './useSelection'

interface SelectBadgeProps extends CardSelection {
  /** entity title, used for the accessible label */
  label: string
}

/** Corner checkbox overlaid on a media card cover; the parent Link is told not to navigate on click. */
export function SelectBadge({ active, selected, onToggle, label }: SelectBadgeProps) {
  const { t } = useTranslation('media')
  return (
    <button
      type="button"
      aria-label={selected ? t('select.deselect', { title: label }) : t('select.select', { title: label })}
      aria-pressed={selected}
      onClick={(e) => {
        e.preventDefault()
        e.stopPropagation()
        onToggle(e.shiftKey)
      }}
      className={cn(
        'absolute top-1.5 left-1.5 flex size-6 cursor-pointer items-center justify-center rounded-full border transition-opacity',
        selected
          ? 'border-accent bg-accent text-accent-ink opacity-100'
          : 'border-white/70 bg-black/40 backdrop-blur-sm hover:border-white',
        active ? 'opacity-100' : 'opacity-0 group-hover:opacity-100 focus-visible:opacity-100',
      )}
    >
      {selected && <Check className="size-3.5" weight="bold" />}
    </button>
  )
}
