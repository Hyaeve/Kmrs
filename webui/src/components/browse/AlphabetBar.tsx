import { useTranslation } from 'react-i18next'
import type { GroupCountDto } from '@/lib/api/types'
import { LETTERS } from '@/components/filters/types'
import { cn } from '@/lib/utils/cn'

interface AlphabetBarProps {
  /** first-character groups from seriesApi.alphabeticalGroups; bar hides itself when empty */
  groups?: GroupCountDto[]
  /** currently active letter filter ('A'..'Z' or '#') */
  active?: string
  onSelect: (letter: string) => void
}

/** A-Z jump bar; letters without any series are shown dimmed and disabled. */
export function AlphabetBar({ groups, active, onSelect }: AlphabetBarProps) {
  const { t } = useTranslation('browse')
  if (!groups || groups.length === 0) return null
  const counts = new Map(groups.map((g) => [g.group, g.count]))
  const others = groups.filter((g) => !/^[a-z]$/.test(g.group)).reduce((n, g) => n + g.count, 0)

  const button = (key: string, label: string, count: number) => {
    const isActive = active === key
    return (
      <button
        key={key}
        type="button"
        disabled={count === 0}
        aria-pressed={isActive}
        title={count > 0 ? t('seriesCount', { count }) : undefined}
        onClick={() => onSelect(key)}
        className={cn(
          'flex h-7 min-w-7 cursor-pointer items-center justify-center rounded-md px-1 font-mono text-xs transition-colors',
          isActive
            ? 'bg-accent-soft text-accent-strong'
            : count === 0
              ? 'cursor-default text-ink-3/40'
              : 'text-ink-3 hover:bg-raised hover:text-ink',
        )}
      >
        {label}
      </button>
    )
  }

  return (
    <nav aria-label={t('alphabet.ariaLabel')} className="mb-5 flex flex-wrap items-center gap-0.5">
      {LETTERS.map((l) => button(l, l, counts.get(l.toLowerCase()) ?? 0))}
      {others > 0 && button('#', '#', others)}
    </nav>
  )
}
