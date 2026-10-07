import { Link } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { EmptyState } from '@/components/ui/EmptyState'
import { Button } from '@/components/ui/Button'
import { Compass } from '@phosphor-icons/react'

export function NotFoundPage() {
  const { t } = useTranslation()
  return (
    <EmptyState
      className="py-32"
      icon={<Compass />}
      title={t('notFound.title')}
      body={t('notFound.body')}
      action={
        <Link to="/dashboard">
          <Button variant="primary">{t('notFound.back')}</Button>
        </Link>
      }
    />
  )
}
