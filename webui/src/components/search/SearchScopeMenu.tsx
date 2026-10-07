import { useQuery } from '@tanstack/react-query'
import { useLocation } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { BookBookmark, Books, CaretDown, Check, PushPin } from '@phosphor-icons/react'
import { librariesApi } from '@/lib/api/libraries'
import { usePinnedLibraries } from '@/lib/store/clientSettings'
import { scopeValue, useSearchScope, useSearchScopeDefault } from '@/components/search/scope'
import { Button } from '@/components/ui/Button'
import { Menu, MenuItem, MenuSeparator } from '@/components/ui/Menu'

export function SearchScopeMenu() {
  const { t } = useTranslation('search')
  const { data: libraries } = useQuery({ queryKey: ['libraries'], queryFn: librariesApi.list })
  const { pinned } = usePinnedLibraries()
  // on /search the ?scope= param wins (shareable, back/forward); everywhere
  // else the menu shows the persisted default that the next search will use
  const onSearchPage = useLocation().pathname.startsWith('/search')
  const { scope, setScope } = useSearchScope()
  const scopeDefault = useSearchScopeDefault()
  const value = onSearchPage ? scopeValue(scope) : scopeDefault.value

  const onScopeChange = (v: string) => {
    scopeDefault.setValue(v)
    if (onSearchPage) setScope(v)
  }

  // pinned libraries float to the top, same as the sidebar
  const pinnedIndex = new Map((pinned ?? []).map((id, i) => [id, i]))
  const pinRank = (id: string) => pinnedIndex.get(id) ?? Number.MAX_SAFE_INTEGER
  const sorted = libraries?.slice().sort((a, b) => pinRank(a.id) - pinRank(b.id))
  const hasPinned = (pinned?.length ?? 0) > 0
  const current = value !== 'all' && value !== 'pinned' ? libraries?.find((l) => l.id === value) : undefined
  const label = value === 'all' ? t('scope.all') : value === 'pinned' ? t('scope.pinned') : (current?.name ?? t('scope.library'))

  return (
    <Menu
      align="start"
      trigger={
        <Button
          variant="secondary"
          size="sm"
          className="h-9 max-sm:px-2.5"
          aria-label={t('scope.ariaLabel', { label })}
        >
          {value === 'pinned' ? (
            <PushPin weight="fill" className="size-4 shrink-0" />
          ) : value === 'all' ? (
            <Books className="size-4 shrink-0" />
          ) : (
            <BookBookmark className="size-4 shrink-0" />
          )}
          <span className="max-w-40 truncate max-sm:hidden">{label}</span>
          <CaretDown className="size-3.5 shrink-0 text-ink-3 max-sm:hidden" />
        </Button>
      }
    >
      <MenuItem onSelect={() => onScopeChange('all')}>
        <span className="flex-1">{t('scope.all')}</span>
        {value === 'all' && <Check className="size-4 text-accent" />}
      </MenuItem>
      {hasPinned && (
        <MenuItem onSelect={() => onScopeChange('pinned')}>
          <PushPin weight="fill" className="size-3.5 text-ink-3" />
          <span className="flex-1">{t('scope.pinned')}</span>
          {value === 'pinned' && <Check className="size-4 text-accent" />}
        </MenuItem>
      )}
      {sorted && sorted.length > 0 && <MenuSeparator />}
      {sorted?.map((l) => (
        <MenuItem key={l.id} onSelect={() => onScopeChange(l.id)}>
          <span className="flex-1 truncate">{l.name}</span>
          {pinnedIndex.has(l.id) && <PushPin weight="fill" className="size-3 shrink-0 text-ink-3" />}
          {value === l.id && <Check className="size-4 text-accent" />}
        </MenuItem>
      ))}
    </Menu>
  )
}
