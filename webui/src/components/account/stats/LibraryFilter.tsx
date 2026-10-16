import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Books, CaretDown, Check } from '@phosphor-icons/react'
import { librariesApi } from '@/lib/api/libraries'
import { Button } from '@/components/ui/Button'
import { Menu, MenuItem, MenuSeparator } from '@/components/ui/Menu'

export function LibraryFilter({ value, onChange }: { value?: string; onChange: (v?: string) => void }) {
  const { t } = useTranslation('stats')
  const { data: libraries } = useQuery({ queryKey: ['libraries'], queryFn: librariesApi.list })
  const label = (value ? libraries?.find((l) => l.id === value)?.name : undefined) ?? t('library.all')

  return (
    <Menu
      trigger={
        <Button variant="secondary" size="sm" aria-label={t('library.ariaLabel', { label })}>
          <Books className="size-4 shrink-0" />
          <span className="max-w-40 truncate">{label}</span>
          <CaretDown className="size-3.5 shrink-0 text-ink-3" />
        </Button>
      }
    >
      <MenuItem onSelect={() => onChange(undefined)}>
        <span className="flex-1">{t('library.all')}</span>
        {!value && <Check className="size-4 text-accent" />}
      </MenuItem>
      {libraries && libraries.length > 0 && <MenuSeparator />}
      {libraries?.map((l) => (
        <MenuItem key={l.id} onSelect={() => onChange(l.id)}>
          <span className="flex-1 truncate">{l.name}</span>
          {value === l.id && <Check className="size-4 text-accent" />}
        </MenuItem>
      ))}
    </Menu>
  )
}
