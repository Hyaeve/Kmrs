import { useEffect, useState } from 'react'
import { Trans, useTranslation } from 'react-i18next'
import { Funnel, MagnifyingGlass, X } from '@phosphor-icons/react'
import { Button } from '@/components/ui/Button'
import { describeActiveFilters, parseAuthor, type ActiveFilterItem } from './filterUrl'
import { SortMenu } from './SortMenu'
import type { AuthorFilter, FilterGroupDef, FilterState, GroupKey, SortOption, SortState } from './types'

interface FilterBarProps {
  count?: number
  noun: 'books' | 'series'
  groups: FilterGroupDef[]
  state: FilterState
  activeCount: number
  onToggleValue: (key: GroupKey, value: string) => void
  onToggleAuthor: (author: AuthorFilter) => void
  onQChange: (q: string) => void
  onOpenFilters: () => void
  sortOptions: SortOption[]
  sort: SortState
  onSortChange: (s: SortState) => void
}

function FilterChip({ label, onRemove }: { label: string; onRemove: () => void }) {
  const { t } = useTranslation('filters')
  return (
    <button
      type="button"
      onClick={onRemove}
      title={t('chip.remove', { label })}
      className="inline-flex max-w-56 cursor-pointer items-center gap-1 rounded-full border border-line bg-raised px-2.5 py-1 text-xs text-ink-2 transition-colors hover:border-line-strong hover:text-ink"
    >
      <span className="truncate">{label}</span>
      <X className="size-3 shrink-0 text-ink-3" />
    </button>
  )
}

export function FilterBar({
  count,
  noun,
  groups,
  state,
  activeCount,
  onToggleValue,
  onToggleAuthor,
  onQChange,
  onOpenFilters,
  sortOptions,
  sort,
  onSortChange,
}: FilterBarProps) {
  const { t } = useTranslation('filters')
  // the search box represents q, so the q chip would just duplicate it
  type ChipItem = ActiveFilterItem & { group: GroupKey }
  const items = describeActiveFilters(state, groups).filter((i): i is ChipItem => i.group !== 'q')
  const [text, setText] = useState(state.q)
  // external changes (shared links, clear-all) resync the input during render
  const [prevQ, setPrevQ] = useState(state.q)
  if (prevQ !== state.q) {
    setPrevQ(state.q)
    setText(state.q)
  }

  useEffect(() => {
    if (text === state.q) return
    const t = setTimeout(() => onQChange(text), 300)
    return () => clearTimeout(t)
  }, [text, state.q, onQChange])

  const remove = (item: ChipItem) => {
    if (item.group === 'authors') onToggleAuthor(parseAuthor(item.value))
    else onToggleValue(item.group, item.value)
  }

  return (
    <div className="mb-5 flex flex-wrap items-center gap-x-3 gap-y-2">
      <span className="text-xs text-ink-3">
        {count !== undefined && (
          <Trans i18nKey={`filters:noun.${noun}`} count={count} components={{ num: <span className="font-mono" /> }} />
        )}
      </span>
      <div className="flex min-w-0 flex-1 flex-wrap items-center justify-end gap-2">
        <div className="relative w-full max-w-56">
          <MagnifyingGlass className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-ink-3" />
          <input
            type="search"
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder={t(`search.${noun}`)}
            className="h-8 w-full rounded-lg border border-line bg-surface pr-3 pl-9 text-sm text-ink transition-colors placeholder:text-ink-3 focus:border-accent/70 focus:outline-none"
          />
        </div>
        {items.map((item) => (
          <FilterChip key={item.key} label={item.label} onRemove={() => remove(item)} />
        ))}
        <Button variant="secondary" size="sm" onClick={onOpenFilters}>
          <Funnel className="size-4" />
          {t('title')}
          {activeCount > 0 && (
            <span className="flex h-4 min-w-4 items-center justify-center rounded-full bg-accent px-1 font-mono text-[10px] font-semibold text-accent-ink">
              {activeCount}
            </span>
          )}
        </Button>
        <SortMenu options={sortOptions} value={sort} onChange={onSortChange} />
      </div>
    </div>
  )
}
