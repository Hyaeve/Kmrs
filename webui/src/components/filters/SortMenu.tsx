import { useTranslation } from 'react-i18next'
import { ArrowDown, ArrowsDownUp, ArrowUp, Check } from '@phosphor-icons/react'
import { Button } from '@/components/ui/Button'
import { Menu, MenuItem, MenuLabel, MenuSeparator } from '@/components/ui/Menu'
import { defaultDirection, sortLabel } from './sort'
import type { SortOption, SortState } from './types'

interface SortMenuProps {
  options: SortOption[]
  value: SortState
  onChange: (s: SortState) => void
}

export function SortMenu({ options, value, onChange }: SortMenuProps) {
  const { t } = useTranslation('filters')
  return (
    <Menu
      trigger={
        <Button variant="secondary" size="sm">
          <ArrowsDownUp className="size-4" />
          {sortLabel(options, value.property)}
          {value.direction === 'asc' ? (
            <ArrowUp className="size-3.5 text-ink-3" />
          ) : (
            <ArrowDown className="size-3.5 text-ink-3" />
          )}
        </Button>
      }
    >
      <MenuLabel>{t('sort.sortBy')}</MenuLabel>
      {options.map((o) => {
        const active = o.property === value.property
        return (
          <MenuItem
            key={o.property}
            onSelect={() =>
              // re-picking the active field flips its direction
              onChange({
                property: o.property,
                direction: active ? (value.direction === 'asc' ? 'desc' : 'asc') : defaultDirection(o.property),
              })
            }
          >
            <span className="flex-1">{t(o.labelKey)}</span>
            {active && <Check className="size-4 text-accent" />}
          </MenuItem>
        )
      })}
      <MenuSeparator />
      <MenuItem onSelect={() => onChange({ ...value, direction: 'asc' })}>
        <ArrowUp className="size-4" />
        <span className="flex-1">{t('sort.ascending')}</span>
        {value.direction === 'asc' && <Check className="size-4 text-accent" />}
      </MenuItem>
      <MenuItem onSelect={() => onChange({ ...value, direction: 'desc' })}>
        <ArrowDown className="size-4" />
        <span className="flex-1">{t('sort.descending')}</span>
        {value.direction === 'desc' && <Check className="size-4 text-accent" />}
      </MenuItem>
    </Menu>
  )
}
