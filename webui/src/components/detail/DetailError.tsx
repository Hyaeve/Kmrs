import { useTranslation } from 'react-i18next'
import { ApiError } from '@/lib/api/client'
import { Compass, Warning } from '@phosphor-icons/react'
import { Button } from '@/components/ui/Button'
import { EmptyState } from '@/components/ui/EmptyState'

interface DetailErrorProps {
  error: unknown
  notFoundTitle: string
  onRetry: () => void
}

export function DetailError({ error, notFoundTitle, onRetry }: DetailErrorProps) {
  const { t } = useTranslation('detail')
  if (error instanceof ApiError && error.status === 404) {
    return (
      <EmptyState
        className="py-32"
        icon={<Compass />}
        title={notFoundTitle}
        body={t('error.notFoundBody')}
      />
    )
  }
  return (
    <EmptyState
      className="py-32"
      icon={<Warning />}
      title={t('error.title')}
      body={error instanceof Error ? error.message : t('error.unknown')}
      action={
        <Button variant="secondary" onClick={onRetry}>
          {t('common:action.retry')}
        </Button>
      }
    />
  )
}
