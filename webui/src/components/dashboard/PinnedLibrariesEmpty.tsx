import { useTranslation } from 'react-i18next'
import { PushPin } from '@phosphor-icons/react'
import { EmptyState } from '@/components/ui/EmptyState'
import { Button } from '@/components/ui/Button'

export function PinnedLibrariesEmpty({ onManage }: { onManage: () => void }) {
  const { t } = useTranslation('dashboard')
  return (
    <EmptyState
      icon={<PushPin />}
      title={t('pinnedEmpty.title')}
      body={t('pinnedEmpty.body')}
      action={
        <Button variant="primary" onClick={onManage}>
          {t('pinnedEmpty.action')}
        </Button>
      }
    />
  )
}
