import { useTranslation } from 'react-i18next'
import { CaretDown, Check, Globe } from '@phosphor-icons/react'
import { SUPPORTED_LANGUAGES } from '@/lib/i18n'
import { Menu, MenuItem } from '@/components/ui/Menu'
import { cn } from '@/lib/utils/cn'

export function LanguageSwitcher({ className, size = 'md' }: { className?: string; size?: 'sm' | 'md' }) {
  const { i18n } = useTranslation()
  const current = SUPPORTED_LANGUAGES.find((l) => l.code === i18n.language) ?? SUPPORTED_LANGUAGES[0]
  return (
    <Menu
      trigger={
        <button
          type="button"
          className={cn(
            'flex cursor-pointer items-center gap-1.5 rounded-lg border border-line bg-surface font-medium whitespace-nowrap text-ink-3 transition-colors hover:text-ink-2',
            size === 'sm' ? 'px-2.5 py-1 text-xs' : 'px-3 py-1.5 text-[13px]',
            className,
          )}
        >
          <Globe size={size === 'sm' ? 14 : 15} />
          {current.label}
          <CaretDown size={size === 'sm' ? 10 : 12} />
        </button>
      }
    >
      <div className="max-h-80 overflow-y-auto">
        {SUPPORTED_LANGUAGES.map((l) => (
          <MenuItem key={l.code} onSelect={() => void i18n.changeLanguage(l.code)}>
            {l.label}
            {l.code === current.code && <Check size={14} className="ml-auto" />}
          </MenuItem>
        ))}
      </div>
    </Menu>
  )
}
