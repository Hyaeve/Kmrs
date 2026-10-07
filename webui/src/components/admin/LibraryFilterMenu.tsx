import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Books, CaretDown, Check } from '@phosphor-icons/react'
import { librariesApi } from '@/lib/api/libraries'
import { Button } from '@/components/ui/Button'
import { Menu, MenuItem } from '@/components/ui/Menu'

interface LibraryFilterMenuProps {
  value: string | null
  onChange: (libraryId: string | null) => void
}

export function LibraryFilterMenu({ value, onChange }: LibraryFilterMenuProps) {
  const { t } = useTranslation('admin-maintenance')
  const { data: libraries } = useQuery({ queryKey: ['libraries'], queryFn: librariesApi.list })
  const current = libraries?.find((l) => l.id === value)

  return (
    <Menu
      trigger={
        <Button variant="secondary" size="sm">
          <Books className="size-4" />
          {value ? (current?.name ?? t('browse:libraryFallback')) : t('account:profile.allLibraries')}
          <CaretDown className="size-3.5 text-ink-3" />
        </Button>
      }
    >
      <MenuItem onSelect={() => onChange(null)}>
        <span className="flex-1">{t('account:profile.allLibraries')}</span>
        {value === null && <Check className="size-4 text-accent" />}
      </MenuItem>
      {(libraries ?? []).map((l) => (
        <MenuItem key={l.id} onSelect={() => onChange(l.id)}>
          <span className="flex-1">{l.name}</span>
          {value === l.id && <Check className="size-4 text-accent" />}
        </MenuItem>
      ))}
    </Menu>
  )
}
