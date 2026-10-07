import { DownloadSimple } from '@phosphor-icons/react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils/cn'

/** Anchor version of the secondary Button: a real <a download> is needed for file downloads. */
export function DownloadLink({ href, className, disabled }: { href: string; className?: string; disabled?: boolean }) {
  const { t } = useTranslation()
  return (
    <a
      href={disabled ? undefined : href}
      download
      aria-disabled={disabled || undefined}
      className={cn(
        'inline-flex h-10 shrink-0 cursor-pointer items-center justify-center gap-2 rounded-lg border border-line bg-raised px-4 text-sm whitespace-nowrap text-ink transition-all duration-150',
        'hover:border-line-strong hover:bg-overlay active:scale-[0.98]',
        disabled && 'pointer-events-none opacity-50',
        className,
      )}
    >
      <DownloadSimple className="size-4" />
      {t('action.download')}
    </a>
  )
}
