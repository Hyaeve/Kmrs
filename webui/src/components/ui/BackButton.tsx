import { useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { ArrowLeft } from '@phosphor-icons/react'
import { cn } from '@/lib/utils/cn'
import { IconButton } from '@/components/ui/IconButton'

/** navigate(-1) only works with in-app history; direct loads fall back to `to`. */
export function BackButton({ to, className }: { to: string; className?: string }) {
  const navigate = useNavigate()
  const { t } = useTranslation()
  return (
    <IconButton
      label={t('action.back')}
      // above DetailHero's blurred backdrop, which bleeds upward over this spot
      className={cn('relative z-10', className)}
      onClick={() => {
        if ((window.history.state?.idx ?? 0) > 0) navigate(-1)
        else navigate(to)
      }}
    >
      <ArrowLeft className="size-5" />
    </IconButton>
  )
}

/**
 * For top-level pages (browse, search) that are also link targets from other pages:
 * renders nothing on a direct load, where there is nothing to go back to.
 */
export function HistoryBackButton({ to, className }: { to: string; className?: string }) {
  if ((window.history.state?.idx ?? 0) <= 0) return null
  return <BackButton to={to} className={className} />
}
